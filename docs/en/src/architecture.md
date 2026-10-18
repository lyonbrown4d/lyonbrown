# Components and calls

`agentd` owns sessions, tasks, models, tools and permissions. `agent-client` puts a bearer token in gRPC request metadata; terminal and desktop clients use the same `ClientService` messages. Concrete `rpi-harness` types stay within the runtime adapter and do not enter public protocol or UI models.

## System overview

```mermaid
flowchart TB
    subgraph Clients[Local clients]
        CLI[Ratatui TUI]
        Desktop[GPUI Desktop / Markdown / Editor / Diff]
        Launcher[agent-launch / native service / version negotiation]
        Client[agent-client / typed authenticated gRPC]
        CLI --> Launcher
        Desktop --> Launcher
        CLI --> Client
        Desktop --> Client
        Browser[Embedded BrowserRuntime / CEF]
        CLI --> Browser
        Desktop --> Browser
    end
    subgraph Agentd[Local agentd / FluxDI composition]
        API[Client / Workspace / Browser / Orchestration services]
        Decision[Decision Engine / rules and structured classification]
        Run[Run lifecycle / concurrency / cancellation / recovery]
        Harness[agent-runtime / sole rpi-harness loop]
        Model[Model registry / metadata and capability limits]
        Tools[Workspace / command / MCP / browser / delegation tools]
        Approval[Exact approval / owner_epoch / durable intent]
        Store[Diesel / SQLite / Session / Task / Event / Outbox]
        Dispatcher[Master Dispatcher / PeerManager]
        API --> Run
        API --> Dispatcher
        Run --> Decision
        Run --> Harness
        Harness --> Model
        Harness --> Tools
        Tools --> Approval
        Tools --> Dispatcher
        Approval --> Store
        Run --> Store
        Dispatcher --> Store
    end
    Client --> API
    Launcher --> Agentd
    Browser <-->|Authenticated attachment / commands and frames| API
    Model --> Remote[OpenAI Chat / Responses / Anthropic Messages]
    Model --> Native[llama-cpp-2 / mistral.rs / local CPU models]
    Tools --> Workspace[Workspace / Git / artifacts / notify]
    Tools --> MCP[rmcp / stdio / Streamable HTTP]
    subgraph PeerNode[Trusted Worker node]
        Worker[PeerService / TaskService / isolated Harness Session]
        WorkerStore[Worker SQLite and isolated workspace]
        Worker --> WorkerStore
    end
    Dispatcher <-->|Paired identity / mTLS / TaskEnvelope| Worker
    subgraph WAN[Self-hosted control plane]
        Control[agent-control / directory and authorization]
        Relay[Opaque Relay frame forwarding]
        Control --> Relay
    end
    MasterRelay[Master RelayTransport]
    WorkerRelay[Worker RelayTransport]
    Dispatcher <-->|Explicit relay route / inner mTLS| MasterRelay
    Worker <-->|Inner mTLS / same task protocol| WorkerRelay
    MasterRelay <-->|Noise IK / outbound mTLS| Relay
    WorkerRelay <-->|Noise IK / outbound mTLS| Relay
```

Typed FluxDI modules inject configuration, identity, storage, models, decision policy and runtime resources. Business modules receive explicit dependencies; clients cannot create model connections, approval decisions or Worker trust records.

The control directory, Noise links and TaskService are separate protocol boundaries. Configuration selects direct or relayed routes before dispatch; a failed operation never triggers automatic route fallback. Relay transports the same mutual-TLS task protocol, preserving Worker identity, capability and owner checks. Control forwards encrypted frames without interpreting tasks or storing Agent Sessions. An encrypted connection alone does not establish task delivery or execution.

## Runs and side effects

```mermaid
sequenceDiagram
    participant C as CLI / Desktop
    participant D as agentd
    participant S as SQLite
    participant H as Sole Harness
    participant T as Tool adapter
    C->>D: StartRun / Session / epoch / idempotency_key
    D->>S: Atomic Run, event and Outbox commit
    D->>D: Decide and restrict tools, models and targets
    D->>S: Persist effective execution plan
    D->>H: Execute / stream domain events
    H->>T: ToolCall / call_id
    T->>S: Request exact-action approval
    D-->>C: Approval / arguments and expiry
    C->>D: Allow once or deny
    T->>S: Consume matching approval / persist intent
    T->>T: Authorized side effect / cancellation and deadline
    T->>S: Persist receipt / Uncertain
    H->>S: Persist Harness session semantics
    D-->>C: Domain events / cursor recovery
```

Approval cannot broaden the execution plan. Command and browser actions record intent and outcome under stable action identifiers. A disconnect, cancellation or process exit that leaves an unknown outcome produces `Uncertain`; reconciliation uses the same identity without automatic replay. File writes, MCP and other tools follow their own adapter authorization and error contracts; one tool's receipt guarantees do not apply to every tool.

## Models and distributed tasks

Remote model protocols, local inference engines and decision classifiers share domain boundaries. Model metadata determines tool, image and context support. Host policy, available capabilities and Worker grants further restrict decision output. Local models load from explicitly configured paths without automatic downloads or cloud fallback.

`OrchestrationService` serves authenticated local clients; peer `TaskService` serves paired mTLS nodes. Dispatch transactions validate Master identity, Session, `owner_epoch`, parent Run and workspace, and persist the Worker binding before sending. Clients page, reconcile or cancel using task identities. Worker success leaves the Master task `awaiting_acceptance`; only explicit acceptance changes it to `completed`.

### Local execution details

```mermaid
flowchart TB
    subgraph Clients[Presentation and input]
        CLI[Ratatui CLI]
        Desktop[GPUI Desktop]
        Client[agent-client]
        CLI --> Client
        Desktop --> Client
    end
    subgraph Daemon[agentd]
        Service[Authenticated gRPC service]
        Orchestration[Session and task orchestration]
        Runtime[agent-runtime]
        Harness[rpi-harness sole agent loop]
        Tools[Host authorization and tools]
        Store[agent-storage SQLite]
        Service --> Orchestration
        Orchestration --> Runtime
        Runtime --> Harness
        Harness --> Tools
        Orchestration --> Store
        Runtime --> Store
    end
    Client -->|Bearer token / HTTP2| Service
    Harness -->|Model requests| Provider[Model provider]
    Tools --> Workspace[Workspace and platform capabilities]
```

Model interaction, tool execution and durable execution state belong to the daemon. Clients cannot infer that a tool side effect happened from UI state, or replay a request simply because its event stream disconnected.

## Browser and device boundaries

`agent-browser-runtime` is embedded in CLI/Desktop. CEF helpers start through a private mode of the client executable. The runtime attaches to the daemon through an authenticated bidirectional stream; the daemon authorizes browser commands before dispatch. GPUI window state and browser execution state remain separate.

```mermaid
flowchart LR
    Client[CLI / GPUI Desktop] -->|Authenticated client protocol| Daemon[Local agentd]
    Runtime[Embedded BrowserRuntime] -->|Authenticated bidirectional attachment| Daemon
    Daemon -->|Authorized commands| Runtime
    Runtime --> Helper[Client private-mode CEF helper]
    Helper --> Web[Untrusted web content]
    Daemon --> Local[Local SQLite]
    Daemon -->|TaskService / PeerService| Worker[Trusted worker daemon]
    Worker --> Remote[Worker local persistence]
```

Disconnecting the browser attachment does not replay actions. Web content cannot access privileged application interfaces. Profiles preserve browser data on disk, without restoring live in-memory pages. Node discovery grants no execution authority; pairing and task authorization remain with the daemon. Public registration and relay use a separate control-plane protocol; discovery does not establish trust.

## Source references

- Services and messages: `proto/agent/v1/agent.proto`.
- Authenticated connection: `crates/agent-client/src/lib.rs`.
- Runtime interfaces: `crates/agent-runtime/src/contracts.rs`.
- Durable state: `crates/agent-storage/src/schema.sql`.

A successful worker attempt and a master's acceptance of its result have different meanings. The protocol keeps `attempt_id`, `owner_epoch` and explicit `AcceptResult`; receiving a result does not establish acceptance.
