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
        API[Client / Model / Skill / Workspace / Runtime / Orchestration services]
        Decision[agent-decision / Agent tool and node policy]
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
    subgraph NativeHost[Interactive-user host interface]
        Computer[agent-computer-runtime / embedded library]
        OS[agent-computer / OS session and permissions]
        Computer --> OS
    end
    Computer <-->|Authenticated session attachment / observe and approved execution| API
    SharedDecision[decision-core / reusable rules and backend ports]
    Decision --> SharedDecision
    subgraph GatewayService[Independent AI Gateway]
        GatewayHTTP[FluxDI Axum / Bearer API / Swagger]
        GatewayUI[Leptos SSR / admin sessions]
        GatewayStore[Diesel SQLite / policy / audit / credential hashes]
        GatewayHTTP --> SharedDecision
        GatewayHTTP --> GatewayStore
        GatewayUI --> GatewayStore
        GatewayUI --> SharedDecision
    end
    Model -.->|Explicit gateway endpoint| GatewayHTTP
    GatewayHTTP --> GatewayUpstream[Chat Completions upstream]
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

A request can involve several model generations and tool calls, while the same daemon-owned Harness manages history and the loop. The model adapter can use a standard API or local inference backend; clients receive domain events only.

```mermaid
sequenceDiagram
    participant C as CLI / Desktop
    participant D as agentd
    participant S as Local SQLite
    participant H as Sole rpi-harness
    participant P as Model adapter
    participant T as Host tool adapter
    C->>D: StartRun / Session / epoch / idempotency key
    D->>S: Atomically save Run, binding, events and Outbox
    D-->>C: Accepted Run ID
    D->>D: Restrict model, tool and node authority
    D->>S: Persist effective execution_plan
    D->>H: Open session and execute prompt
    loop Further model generation is needed
        H->>P: Message history and permitted tool schemas
        P-->>H: Text or structured tool_calls
        opt Response contains tool calls
            H->>T: Tool name, call_id and arguments
            T->>T: Check tool, arguments, workspace and cancellation
            opt Action requires human approval
                T->>S: Persist pending approval
                D-->>C: Approval event / exact arguments and expiry
                C->>D: Allow once or deny
            end
            alt Authorization and validation pass
                opt Durable command, browser or computer action
                    T->>S: Consume matching approval and persist intent
                end
                T->>T: Execute permitted operation
                T-->>H: Tool result or execution error
            else Denied, expired or mismatched arguments
                T-->>H: Denial result without executing the action
            end
        end
        H->>S: Persist messages, tool records and related events
        D-->>C: WatchRun / text, tool and approval events
    end
    D->>S: Persist terminal Run, events and Outbox
    D-->>C: Run result and event cursor
```

Source references: [apps/agentd/src/execution.rs](https://github.com/lyonbrown4d/lyonbrown/blob/705f995a59ff69e186f07a035dcb639e43f9d44d/apps/agentd/src/execution.rs), [crates/agent-runtime/src/runtime.rs](https://github.com/lyonbrown4d/lyonbrown/blob/705f995a59ff69e186f07a035dcb639e43f9d44d/crates/agent-runtime/src/runtime.rs), [crates/agent-runtime/src/tools.rs](https://github.com/lyonbrown4d/lyonbrown/blob/705f995a59ff69e186f07a035dcb639e43f9d44d/crates/agent-runtime/src/tools.rs).

Approval cannot broaden the execution plan. Command, browser and computer actions record intent and outcome under stable action identifiers. A disconnect, cancellation or process exit that leaves an unknown outcome produces `Uncertain`; reconciliation uses the same identity without automatic replay. File writes, MCP and other tools follow their own adapter authorization and error contracts; one tool's receipt guarantees do not apply to every tool.

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
    Computer[ComputerRuntime host library] -->|Authenticated fixed session scope| Daemon
    Daemon -->|Observe / prepare / execute after exact approval| Computer
    Computer --> Native[Blocking worker / OS session exclusion]
    Daemon --> Local[Local SQLite]
    Daemon -->|TaskService / PeerService| Worker[Trusted worker daemon]
    Worker --> Remote[Worker local persistence]
```

Disconnecting the browser attachment does not replay actions. Web content cannot access privileged application interfaces. Profiles preserve browser data on disk, without restoring live in-memory pages. Node discovery grants no execution authority; pairing and task authorization remain with the daemon. Public registration and relay use a separate control-plane protocol; discovery does not establish trust.

ComputerRuntime provides an embeddable interactive-user host interface, attached to one fixed daemon session and desktop scope. Native work runs on a blocking worker. Disconnecting cancels commands and retains desktop exclusion until the OS call drains. After checking trusted configuration, session ownership and control permissions, the daemon provides `computer_observe` and `computer_act` through the sole Harness. Observation, immutable action preparation and execution after exact approval use this authenticated connection; approval consumption and intent become durable before execution. CLI/TUI and Desktop use an injected `ComputerHostFactory` to explicitly discover and select a local window or display, then bind the host to the selected daemon session. Switching chats does not transfer an existing attachment. ComputerRuntime is embedded in the client process, without a standalone product process or another Agent Loop. The client `native-computer` feature is disabled by default; wired entry points do not establish native-control runtime acceptance. On macOS, the policy requires a logged-in console session, a valid session UUID and an explicit unlocked field before proceeding to permission checks. Unknown or locked state is rejected; an observed session failure cannot recover automatically after unlocking. See [Native desktop execution boundary](protocol.md#native-desktop-execution-boundary) for permissions, leases and execution limits, and [model computer tools](tools.md#model-computer-tools) for tool behavior.

## Source references

- Services and messages: `proto/agent/v1/agent.proto`.
- Authenticated connection: `crates/agent-client/src/lib.rs`.
- Runtime interfaces: `crates/agent-runtime/src/contracts.rs`.
- Native host attachment: `apps/agent-cli/src/computer_backend.rs`, `apps/agent-desktop/src/computer.rs`, `crates/agent-computer-runtime/src/client.rs`, `crates/agent-computer-runtime/src/attachment.rs`, `apps/agentd/src/computer/service.rs`, `apps/agentd/src/computer/tools/` and `apps/agentd/src/computer/mutation.rs`.
- Durable state: `crates/agent-storage/src/schema.sql`.
- Reusable decisions and Agent integration: `crates/decision-core/`, `crates/decision-semantic/`, `crates/decision-model/` and `apps/agentd/src/model_routing.rs`.
- Independent gateway and synthetic models: `apps/ai-gateway/`, `crates/gateway-core/`, `crates/gateway-storage/`, `crates/gateway-web/` and `apps/model-mock/`.

A successful worker attempt and a master's acceptance of its result have different meanings. The protocol keeps `attempt_id`, `owner_epoch` and explicit `AcceptResult`; receiving a result does not establish acceptance.
