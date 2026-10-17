---
title: System Architecture Overview
---

Lyonbrown consists of authenticated clients, a node service called `agentd`, and a task orchestration layer. Clients handle interaction, `agentd` manages execution and persistence on its Node, and Root splits tasks, manages dependencies, and accepts results. Tasks can run on the local Node or another Node; standalone operation and direct node-to-node connections do not depend on Center.

This page describes module responsibilities, data flow, and write authority. The product uses Node.js 24 and TypeScript with `strict` mode enabled.

See [local gRPC C/S](/en/architecture/grpc-client-server/), [composition root](/en/architecture/composition-root/), and [Electron Desktop](/en/architecture/electron-desktop/) for startup, authentication, and client boundaries.

## System overview

The diagrams group local execution, task orchestration, and execution nodes by responsibility. Root is a logical role that can dispatch to its own Node or another Node; these groups do not prescribe packages, microservices, or deployment directories. Dashed lines indicate only optional Center connections.

```mermaid
flowchart TB
  User["User goals and interaction"]

  subgraph Local["Local node · execution and persistence"]
    Client["TUI / Electron Desktop<br/>Desktop: renderer → restricted preload → main<br/>separate Node 24 client worker"]
    Access["agent-launch / agent-client / protocol<br/>discovery, startup, authenticated gRPC<br/>commands and Snapshots"]
    Boundary["agentd · sole composition root<br/>authentication / command FIFO / budgets and epoch"]
    Runtime["agent-runtime / agent-domain<br/>domain events, execution, safe recovery"]
    Decision["DecisionProvider / daemon policy<br/>pre-kernel decision or safe fallback<br/>actions constrained by deterministic policy"]
    Pi["kernel-pi<br/>Pi execution-engine adapter"]
    Model["local-model<br/>model lifecycle and inference"]
    Tools["Tool policy and approval<br/>files / search / approved processes"]
    Native["Typed native capabilities<br/>Windows / POSIX adapters"]
    Store[("EventStore · SQLite<br/>session and execution source of truth")]
    Cache["Model caches<br/>GGUF / separate decision bundle"]
    Workspace["Controlled workspace<br/>files and process cwd"]

    Client <--> Access <--> Boundary
    Boundary --> Runtime
    Boundary --> Native
    Runtime -->|"pre-kernel call"| Decision
    Runtime -->|"execute after recording decision"| Pi
    Runtime -->|"persist before projection"| Store
    Pi --> Model --> Cache
    Pi --> Tools --> Workspace
    Native --> Cache
    Native --> Workspace
  end

  subgraph Orchestration["Task orchestration and execution nodes"]
    Root["Root authority<br/>RootPlanner / TaskGraph<br/>single writer / ownerEpoch"]
    Context["ContextSlice / WorkspaceManager<br/>required context, fixed baseline, isolated workspaces"]
    Dispatch["Scheduler / Mesh<br/>deterministic placement, device identity and authorization<br/>versioned task contracts"]
    Admission["NodeAdmission / Executor<br/>atomic slots, leases, Attempts"]
    Execution["Subagent / Tool / Data<br/>isolated sessions, workspaces, local event storage<br/>only agent tasks require an LLM"]
    Results["Root Verifier<br/>results / artifacts / evidence<br/>accepted after verification"]
    Center["Optional Center · optional<br/>registry / relay / backup"]

    Root --> Context --> Dispatch --> Admission --> Execution --> Results
    Center -.-> Dispatch
  end

  User --> Client
  Runtime --> Root
  Results --> Root
  Root --> Store

```

<details>
<summary>Expand the complete module diagram (44 nodes)</summary>

```mermaid
flowchart TB
  U["User"]
  subgraph Local["Local node · Node.js 24 / strict TypeScript"]
    direction TB
    subgraph Clients["Authenticated clients · no execution resources"]
      direction LR
      TUI["TUI · agent-cli"]
      Renderer["Electron renderer<br/>view model / no Node API"]
      Preload["Restricted preload<br/>getState / onState / act / reconnect"]
      Main["Electron main<br/>window and worker lifecycle"]
      Worker["Separate Node 24 client worker"]
      Launch["agent-launch<br/>discovery and startup when needed"]
      Client["agent-client<br/>handshake / authentication / reconnect"]
      View["ui-model<br/>client state projection"]
      Protocol["protocol<br/>versioned local gRPC contract"]
    end
    subgraph Daemon["agentd · sole production composition root"]
      direction LR
      Auth["gRPC service<br/>authentication and boundary validation"]
      FIFO["Command FIFO<br/>budgets / epoch / serialization"]
      ModelState["Model-aware Runtime<br/>model status and activation"]
      Runtime["agent-runtime<br/>durable execution and recovery"]
      Domain["agent-domain<br/>domain events / reducer"]
      Decision["DecisionProvider<br/>typed pre-kernel decisions / safe fallback"]
      Policy["Daemon policy<br/>actions constrained by deterministic policy"]
      Pi["kernel-pi<br/>Pi execution-engine adapter"]
      Model["local-model<br/>lifecycle and inference"]
      Approval["Tool authorization and approval"]
      Tools["Workspace files / search / process tools"]
      Control["local-control-plane<br/>scope / token / lease / discovery"]
      Native["Typed native capabilities<br/>Windows / POSIX and file, process adapters"]
    end
    subgraph Resources["Resources owned or accessed under daemon control"]
      direction LR
      Discovery["Protected discovery<br/>descriptor / token / lease"]
      Store[("EventStore · SQLite WAL<br/>session and execution source of truth")]
      Weights["Model caches<br/>GGUF / separate decision-model bundle"]
      Workspace["Controlled workspace<br/>files and child-process cwd"]
    end
  end
  subgraph Root["Logical Root orchestration"]
    direction LR
    Authority["Root authority<br/>single root-session writer / ownerEpoch"]
    Planner["RootPlanner"]
    Graph["TaskGraph<br/>dependencies / scope / acceptance criteria"]
    Context["ContextSlice<br/>minimal context and authorized references"]
    Workspaces["WorkspaceManager<br/>fixed baseline / isolated workspaces"]
    Scheduler["Deterministic Scheduler"]
    Verifier["Root Verifier<br/>acceptance and integration"]
    Mesh["Mesh<br/>device identity / authorization / versioned tasks"]
  end
  subgraph Remote["Execution Node"]
    direction LR
    Admission["NodeAdmission<br/>atomic reservation of local slots"]
    Executor["Executor<br/>agent / tool / data contract execution"]
    Subagent["Subagent<br/>isolated session / complete Runtime"]
    NodeStore[("Node-local EventStore<br/>Attempts / child sessions / buffered results")]
    TaskWorkspace["Isolated workspace<br/>Git / data / tool resources"]
    Results["Results and artifacts<br/>commits / logs / test or source evidence"]
  end
  subgraph Center["Optional Center · optional"]
    direction LR
    Registry["registry · optional"]
    Relay["relay · optional"]
    Backup["backup · optional"]
  end

  U --> TUI
  U --> Renderer
  Renderer --> Preload --> Main --> Worker
  Worker --> Launch
  Worker --> Client
  TUI --> Launch
  TUI --> Client
  Launch --> Discovery
  Client <-->|"Commands / Snapshots"| Protocol
  Protocol <--> Auth
  Client --> View --> TUI
  Auth --> FIFO --> ModelState --> Runtime
  ModelState --> Auth
  Runtime --> Domain
  Runtime --> Decision --> Policy --> Pi
  Pi --> Model --> Weights
  Decision --> Weights
  Pi --> Approval --> Tools --> Native
  Control --> Auth
  Control --> Discovery
  Control --> Native
  Native --> Workspace
  Runtime -->|"persist domain events first"| Store
  Store -->|"recover"| Runtime

  Runtime --> Authority
  Authority --> Planner --> Graph --> Scheduler
  Graph --> Context
  Graph --> Workspaces
  Context --> Mesh
  Workspaces --> TaskWorkspace
  Scheduler --> Mesh --> Admission --> Executor
  Executor -->|"agent"| Subagent
  Executor -->|"tool / data"| Results
  Subagent --> NodeStore
  Subagent --> TaskWorkspace
  Subagent --> Results --> Verifier --> Authority
  Authority --> Store
  Registry -.-> Mesh
  Relay -.-> Mesh
  NodeStore -.-> Backup
  Store -.-> Backup

```

</details>

Client output paths are simplified in the overview: the daemon's Snapshot stream returns to `agent-client`. The TUI renders through `ui-model`; Desktop receives state through its worker, main process, and restricted preload. Electron main manages windows and the separate Node 24 worker. That worker reuses the public launcher/client interfaces for native discovery and authentication; the renderer does not load the native backend.

`agentd` is the sole composition root within a Node. It owns the lifecycle of the Runtime, models, tools, approvals, MCP connections, and EventStore. Secret and Skills integration follows daemon authorization policy. Third-party Pi and OpenJev types stop at adapters and do not enter public protocols or client view models. Closing a client does not cancel execution already accepted by the daemon.

## Module responsibilities

### Node execution and clients

| Module and source location                                                                       | Input → output                                                                             | Ownership and permission boundary                                                                                                                                                                 |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `apps/agent-cli`, `packages/ui-model`                                                            | User actions, Snapshots → commands, terminal projection                                    | Client; owns neither Runtime nor database.                                                                                                                                                        |
| `apps/agent-desktop/src/{renderer,preload,main}`                                                 | UI actions, restricted IPC, worker state → renderer view model                             | Renderer has no Node API or token. Main manages a separate Node 24 client worker, which reuses public launcher/client interfaces.                                                                 |
| `packages/agent-launch`, `agent-client`, `protocol`                                              | Workspace scope, discovery, commands → daemon connection, versioned Snapshots              | Launcher starts the daemon when needed. Client authenticates with credentials without exposing them to the presentation layer. Local gRPC and node-to-node task contracts are managed separately. |
| `apps/agentd/node-src/composition.ts`, `production-runtime.ts`, `production-kernel.ts`           | Configuration, scope, model lease → Runtime, Kernel, tools, gRPC composition               | Sole production composition root; creates and closes owned resources.                                                                                                                             |
| `apps/agentd/node-src/runtime-command-serial-queue.ts`, `packages/agent-runtime`, `agent-domain` | Validated commands, KernelEvents → domain events, recovered state, Snapshots               | Command FIFO and Runtime persistence enforce their respective ordering rules. Model decisions cannot replace deterministic checks.                                                                |
| `packages/kernel-pi`, `local-model`, `decision-engine`                                           | Model inputs, closed questions, tool steps → normalized execution results, typed decisions | Daemon owns lifecycle. Persist a decision or fallback before continuing Pi.                                                                                                                       |
| `apps/agentd/node-src/{workspace-tools,approval-gate}.ts`                                        | Scoped tool requests, user approvals → file/process results, approval events               | File tools enforce workspace boundaries. `process_execute` requires high-risk approval and runs with host-user privileges; it is not an OS sandbox.                                               |
| `packages/local-control-plane`, `control-plane-native`, `event-store`                            | Native requests, domain events → protected discovery/locks/model storage, SQLite events    | Windows/POSIX differences stay in adapters. File tools also reuse `@openclaw/fs-safe`; system adapters handle process cleanup.                                                                    |

### Task orchestration and node collaboration

| Logical module                                   | Input → output                                                                                        | Authority boundary                                                                                                                                                           |
| ------------------------------------------------ | ----------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| RootPlanner / TaskGraph                          | Goal, baseline, constraints → Tasks with dependencies, scope, budgets, acceptance criteria            | Root manages the plan; dependent Tasks cannot become schedulable before prerequisites are met.                                                                               |
| ContextSlice / WorkspaceManager                  | Task, authorized references → minimal context, isolated workspace, artifact references                | Send only required content. Coding uses a fixed baseline and an independent clone/worktree, without a shared writable directory.                                             |
| Deterministic Scheduler / NodeAdmission          | Ready Task, node capabilities and load → Offer, atomic acceptance, lease                              | Scheduler selects candidates deterministically. The receiving Node checks authorization and reserves its own resources.                                                      |
| Mesh / versioned task contracts                  | Candidate addresses, identity, authorization, task messages → trusted connection, verifiable messages | Discovery does not grant membership; membership does not grant execution permission. Local gRPC tokens cannot simply become Mesh authorization.                              |
| Executor / Subagent / Root Verifier              | Task, ContextSlice, authorization → submitted result and evidence → acceptance or revision request    | Child sessions and workspaces are isolated. Agent tasks use a complete Runtime; tool/data tasks can execute their contracts directly. Only Root acceptance completes a Task. |
| Replication / explicit handoff / optional Center | Authorized events, snapshots, artifacts → replicas or explicit authority transfer                     | Replicas are not writers. Center can provide registry, relay, and backup without determining the default topology.                                                           |

## Single-node commands, execution, and persistence

Single-node execution persists domain events before publishing projections. Initialization and model status can also generate Snapshots; execution results must pass through EventStore before publication.

```mermaid
sequenceDiagram
  autonumber
  actor User as User
  participant Client as Authenticated client
  participant Daemon as agentd authentication / FIFO
  participant Runtime as agent-runtime
  participant Decision as DecisionProvider / daemon policy
  participant Pi as kernel-pi / models and tools
  participant Store as EventStore

  User->>Client: Submit input
  Client->>Daemon: Typed command + session / commandId / expectedServerInstanceId
  Daemon->>Daemon: Authenticate, validate scope/budgets, serialize through FIFO
  Daemon->>Runtime: Accept start-turn
  Runtime->>Store: Append user_message_submitted
  Store-->>Runtime: Persistence succeeded
  Runtime->>Decision: Bounded questions and state
  Decision-->>Runtime: Validated result or safe fallback, continue_pi
  Runtime->>Store: Append decision_recorded or decision_fallback_recorded
  Store-->>Runtime: Persistence succeeded
  Runtime->>Pi: startTurn
  loop Each execution event
    Pi-->>Runtime: Normalized KernelEvent
    Runtime->>Runtime: Map to Lyonbrown domain event
    Runtime->>Store: Append domain event
    Store-->>Runtime: Persistence succeeded
    Runtime-->>Daemon: Publish domain-state projection
    Daemon-->>Client: Snapshot + monotonic transport revision
    Client-->>User: Update interface
  end
```

Daemon-owned deterministic policy and approval gates control tools. When approval is required, the client displays the request and submits the user's decision. Denials also enter domain events; a model cannot expand its permissions. If an EventStore append fails, the Runtime does not project that execution result as success. Snapshot transport revisions and durable event revisions serve separate purposes and are not interchangeable.

Recovery reconstructs domain state and safely resumable history from EventStore. Pi's internal state is not a second authoritative store.

## Task dispatch and cross-node execution

A Node provides physical execution resources. An Agent is a logical execution entity. A Run is one execution of a root goal. A Task is a stable work item; an Attempt is one actual execution of that Task. Retrying creates a new `attemptId`, without recreating the root session or overwriting earlier evidence. The Executor handles `ExecutionKind = agent | tool | data`: agent tasks start a Subagent, while deterministic tool/data work does not require an LLM. All results still require Root verification.

```mermaid
sequenceDiagram
  autonumber
  participant Root as Root authority
  participant RootStore as Root EventStore
  participant Scheduler as Scheduler / Mesh
  participant Admission as Remote NodeAdmission
  participant Executor as Executor
  participant Agent as Subagent
  participant NodeStore as Remote local EventStore
  participant Verifier as Root Verifier

  Root->>RootStore: Save TaskGraph, scope, acceptance criteria, ownerEpoch
  Root->>Scheduler: Ready Task + ContextSlice / WorkspaceRef
  Scheduler->>Admission: TaskOffer: version, identity, authorization, Task/Attempt, idempotency key
  Admission->>Admission: Validate contract, capabilities, budgets, permissions, atomically reserve slots
  alt Rejected or insufficient resources
    Admission-->>Scheduler: Rejection reason, no Attempt starts
  else Accepted
    Admission->>NodeStore: Save admission and lease state
    Admission-->>Scheduler: Accept + lease
    Scheduler-->>Root: Accepted Node / Attempt / lease
    Root->>RootStore: Save assignment and Attempt state
    Admission->>Executor: Start authorized Attempt
    alt agent task
      Executor->>Agent: Start isolated session / workspace
      Agent->>NodeStore: Save local execution events and artifact references
      Agent-->>Executor: Submit candidate result
    else tool / data task
      Executor->>Executor: Execute the task contract directly, without requiring an LLM
      Executor->>NodeStore: Save execution results and artifact references
    end
    Note over Executor,NodeStore: While disconnected, continue only authorized safe steps, buffer results locally
    Executor-->>Root: After reconnection, submit result, artifacts, and evidence
    Root->>Root: Check identity, ownerEpoch, Attempt, lease state, deduplicate
    Root->>Verifier: Check artifacts, test or source evidence, acceptance criteria
    Verifier-->>Root: Accepted or revision request
    Root->>RootStore: Persist acceptance decision before advancing TaskGraph
  end
```

Node-to-node task contracts require versioning and explicit message fields, failure windows, retransmission, and cancellation semantics. The message names in the diagram explain contract responsibilities.

Each Node writes only its local EventStore. Subagents do not directly write the Root Session, and machines do not share one SQLite file. Coding returns commits, patches, and test logs; data tasks return schemas, shards, sources, and checksums. Root sends the necessary ContextSlice without copying its entire conversation, credentials, or all workspace files by default.

Lease expiry does not establish that an old Attempt has stopped or that external side effects have not occurred. Reassignment uses a new Attempt, with retries governed by side-effect idempotency or human review. Results from obsolete authority or invalid Attempts cannot be accepted directly. A disconnected Node preserves results and retransmits them with Task, Attempt, and event identifiers for deduplication. Steps requiring a new Root decision, uncertain authorization, or uncertain side-effect safety pause.

`ownerEpoch` versions root-session write authority and increases through explicit handoff. The local daemon epoch is represented by the protocol field `serverInstanceId` for handshake; commands must provide a matching `expectedServerInstanceId`. A daemon restart is not a transfer of Root authority, and these fields cannot be reused for each other. The originating Node holds Root authority by default. A new Node may continue root-session writes only after state synchronization, target confirmation, and handoff commit. Disconnection or failure to discover the previous Node does not trigger automatic takeover.

Mesh handles secure transport, device identity, and task authorization. Adapters select the underlying transport and LAN/VPN candidate-discovery mechanism. Discovering a Node does not establish trust, and connecting does not grant execution permission. Center can provide registry, relay, and backup, without becoming a prerequisite for standalone startup or direct node-to-node operation.

## Cross-platform capabilities and runtime constraints

Typed capability interfaces provide files, paths, permissions, locks, processes, model storage, and workspace operations. Windows, macOS, and Linux share input/output, authorization, cancellation, timeout, and error semantics. Platform adapters contain system differences and explicitly report missing capabilities.

Windows native share-deny and POSIX advisory locks use different mechanisms. POSIX locks coordinate this adapter's operations; they do not promise to prevent writes by a malicious process running as the same OS user. Process tools run with host-user privileges; approvals and workspace checks do not form an OS sandbox.

Background business scheduling that needs restart durability and retry state requires a job port and a maintained persistent queue or workflow implementation. Native timers and temporary EventStore tables must not become schedulers.

See the [System One decision layer](/en/architecture/system-one-decision-layer/) and related architecture pages for task-model, context, and recovery policies. The [roadmap](/en/plans/roadmap/) and [release artifacts](/en/reference/release-artifacts/) cover delivery order and runtime acceptance records.
