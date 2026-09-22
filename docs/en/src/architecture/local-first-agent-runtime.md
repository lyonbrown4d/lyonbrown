# Lyonbrown Local-First Agent Runtime Architecture

> Version: Draft v0.1  
> Status: Long-term architecture direction; see the [Phase-One MVP Implementation Plan](../plans/mvp-implementation-plan.md) for current implementation progress  
> Positioning: A long-lived, model-provider-neutral, local-first general-purpose Agent Runtime  
> Recommended primary implementation language: Rust

---

## 1. Project Positioning

This project does not aim to build another Coding Agent tied to a particular model or use case. It aims to build a maintainable, general-purpose **Agent Runtime / Agent Kernel**.

Core idea:

> **The Agent owns its identity, context, capabilities, tools, secrets, and working state; models are only replaceable compute resources.**

Models should not own the Agent. The Agent Runtime should own the models.

The system must therefore natively support:

- Local models
- Remote models
- Multi-model collaboration
- Switching models within a session
- Seamless handoffs between models
- Local-first execution
- Remote escalation after a local-model failure
- A small local model as the control model
- A large local model as the primary Worker
- A remote Frontier Model as scarce, high-value compute
- Tools and capabilities independent of model providers
- Agent identity and working style independent of individual sessions
- Secrets never entering model context
- Local-first operation, with an optional official Sync Server and full self-hosting support

---

## 2. Non-Goals

The first phase does not seek to:

- Implement a model inference engine from scratch
- Reimplement llama.cpp / vLLM / Ollama
- Implement a vector database
- Implement a general-purpose network protocol
- Implement every model Provider SDK
- Copy an existing complete or partial Agent Runtime

Reusing foundational libraries is allowed and encouraged, including:

- Tokio
- Axum
- Reqwest
- Serde
- tracing / OpenTelemetry
- Wasmtime
- MCP SDK
- Rig Core or another Provider abstraction library
- Storage libraries such as SQLite / PostgreSQL / RocksDB

Principle:

> **Design the architecture ourselves; reuse mature libraries for foundational capabilities wherever possible.**

---

## 3. Overall Architecture

```text
                           Client Layer
                CLI / Desktop / Web / API / IM
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                       Agent Gateway                         │
│ Auth / Session / Streaming / API / Device                  │
└─────────────────────────────┬───────────────────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Agent Control Plane                     │
│                                                             │
│ Policy Engine                                               │
│ Model Scheduler                                             │
│ Capability Resolver                                         │
│ Budget / Cost                                               │
│ Privacy / Security                                          │
│ Agent Definition                                            │
└─────────────────────────────┬───────────────────────────────┘
                              ▼
╔═════════════════════════════════════════════════════════════╗
║                        Agent Kernel                         ║
║                                                             ║
║ Session                                                     ║
║ Turn                                                        ║
║ Step                                                        ║
║ Agent Loop                                                  ║
║ State Machine                                               ║
║ Event Bus                                                   ║
║ Retry / Cancel / Resume                                     ║
║ Sub Agent                                                   ║
║ Checkpoint / Replay                                         ║
╚══════════════════════╤═══════════════╤══════════════════════╝
                       │               │
          ┌────────────┘               └────────────┐
          ▼                                         ▼
┌─────────────────────────┐              ┌─────────────────────────┐
│      Context Engine     │              │       Model Runtime     │
│                         │              │                         │
│ Working Memory          │              │ Local Control Model     │
│ Long-term Memory        │              │ Local Worker Model      │
│ Retrieval               │              │ Remote Frontier Model   │
│ Compression             │              │ Provider Registry       │
│ Context Budget          │              │ Model Capabilities      │
└────────────┬────────────┘              └────────────┬────────────┘
             │                                        │
             └──────────────────┬─────────────────────┘
                                ▼
┌─────────────────────────────────────────────────────────────┐
│                    Capability Runtime                       │
│ Shell / Filesystem / Browser / Computer Use / Code         │
│ Execution / Git / MCP / HTTP / Database / Custom           │
└─────────────────────────────┬───────────────────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        Tool Runtime                         │
│ Registry / Permission / Execution / Approval / Redaction   │
└─────────────────────────────┬───────────────────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                       Secret Broker                         │
│ SecretRef / Late Binding / Scoped Access / Audit           │
└─────────────────────────────┬───────────────────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Execution Runtime                       │
│ Local Process / Docker / WASM / Remote Worker / Sandbox    │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. Core Principles

### 4.1 Model Provider Neutral

The Agent Runtime must not be tied to OpenAI, Anthropic, Gemini, DeepSeek, Qwen, Ollama, llama.cpp, or vLLM.

Unified abstraction:

```rust
trait ModelProvider {
    async fn stream(&self, request: ModelRequest) -> Result<ModelStream>;
}
```

Model identity must be split into at least:

```text
ProviderId
ModelId
ModelCapabilities
```

Providers must not be hard-coded as a fixed enum.

### 4.2 Agent Sessions Belong to the Runtime, Not the Provider

Wrong:

```text
Session ≈ Claude Conversation
Session ≈ OpenAI Conversation
```

Correct:

```text
Your Session
 ├── Step 1 -> Local Small Model
 ├── Step 2 -> Local Large Model
 ├── Step 3 -> Claude
 ├── Step 4 -> GPT
 └── Step 5 -> Local Large Model
```

Switching models must not lose working style, Workspace state, Skills, the current Plan, Tool state, or user preferences.

---

## 5. Agent Identity Runtime

The Agent's identity and working methods must be independent of any particular Session.

Recommended persisted structure:

```text
AgentProfile
├── persona
├── communication_style
├── working_style
├── coding_preferences
├── tool_preferences
├── reasoning_policy
├── model_preferences
├── privacy_policy
├── default_skills
└── learned_preferences
```

```yaml
id: default-dev
working_style:
  prefer_existing_libraries: true
  avoid_reinventing_wheels: true
  inspect_before_editing: true
  run_tests_after_changes: true
  architecture_first: true
model:
  prefer_local: true
  remote_escalation: true
coding:
  avoid_unnecessary_abstraction: true
  prefer_small_reviewable_changes: true
```

---

## 6. Prompt Compiler

An Agent Profile should not simply be concatenated into the System Prompt.

```text
Agent Profile + Workspace Policy + Skill Instructions + Model Capabilities
                              ↓
                       Prompt Compiler
                              ↓
                  Provider-specific Prompt
```

Different models vary in their sensitivity to System Prompt structure, Tool Calling protocols, Reasoning parameters, Structured Output capabilities, and prompt best practices. The Prompt Compiler must translate a unified Agent Identity into the prompt best suited to each Provider.

---

## 7. Workspace Runtime

New repositories must not require users to create `AGENTS.md` manually. On entry, automatically detect `Cargo.toml`, `go.mod`, `pom.xml`, `build.gradle`, `package.json`, `docker-compose.yml`, `README.md`, Git history, and directory structure.

Generate:

```text
WorkspaceProfile
├── project_type
├── languages
├── frameworks
├── architecture
├── build_commands
├── test_commands
├── conventions
├── enabled_skills
└── repository_instructions
```

Store it by default under `~/.agent/workspaces/<workspace-id>/`, without polluting the Git repository.

### 7.1 Compatibility with Existing Agent Rule Files

Automatically detect:

```text
AGENTS.md
CLAUDE.md
CODEX.md
.github/copilot-instructions.md
.cursor/*
README.md
CONTRIBUTING.md
```

Parse them into `Canonical Repository Instructions`. Recommended precedence:

```text
Session Override > Repository > Workspace > Organization > Global Profile
```

---

## 8. Skill Runtime

Skills should not depend on users manually “invoking” them.

```text
Skill Registry
      ↓
Intent / Capability Matcher
      ↓
Auto Activation
```

```yaml
name: quarkus-debug
triggers: [quarkus, startup failure, extension error]
requires:
  tools: [shell, filesystem]
context:
  files: [pom.xml, build.gradle, application.yaml]
```

A Skill should be a `Global Asset`, not redeclared for every Session.

---

## 9. Model Runtime

The model system should have three layers.

### 9.1 Local Control Model

Recommended: 2B–4B, llama.cpp, CPU/iGPU, short output, Structured Output.

Responsibilities: Intent Classification, Complexity Estimation, Tool Selection, Context Selection, Context Compression, Query Rewrite, Skill Selection, Result Classification, and Escalation Suggestion. It is not primarily responsible for final answers.

### 9.2 Local Worker Model

For example, Qwen 20B–30B through llama.cpp / Ollama / vLLM on a GPU.

Responsibilities: ordinary reasoning, Coding, Debug, Plan, Tool Use, documentation, and local task execution.

> Complete the great majority of ordinary tasks locally whenever possible.

### 9.3 Remote Frontier Model

Examples include Claude, GPT, Gemini, and other highly capable remote models.

Responsibilities: high-complexity reasoning, escalation after local-model failure, large-scale architecture analysis, high-value complex tasks, and multimodal tasks beyond local capabilities.

> Scarce, high-value compute.

---

## 10. Model Scheduler

Use `Step-level Model Scheduling`, not one fixed model per Session. Models are resources scheduled by the Agent Kernel.

```text
Agent Step → Static Policy (Privacy / Capability / Budget / Locality)
           → Complexity Router → Model Selection
```

Selection criteria include Task Capability, Privacy Level, Data Classification, Context Size, Latency, Cost, Local Availability, Model Capability, Current Load, and Historical Success Rate.

---

## 11. Two-Stage Routing

Pre-routing alone is insufficient.

```text
Agent Step → Pre-Execution Router → Selected Model → Result → Quality Gate
                                                        /          \
                                                     PASS        ESCALATE
                                                       ↓             ↓
                                                   Continue     Stronger Model
```

Run the local model first, then determine whether the objective was completed, the format was violated, Tool Results are reasonable, uncertainty appeared, or stronger-model verification is needed. Only then decide whether to escalate.

---

## 12. Context Engine

Context cannot simply be `Vec<Message>`. Define a unified `ContextItem` with types Message, ToolObservation, Document, Memory, Summary, Artifact, Reference, WorkspaceState, Plan, and Finding. Each item contains:

```text
priority
token_estimate
source
timestamp
confidence
expandable
classification
```

### 12.1 Progressive Context Disclosure

Do not send `100K Raw Context → Remote Model` directly. Prefer:

```text
Raw Context → Deterministic Filter → Semantic Retrieval → Local Compression
            → Context Budget → Model
```

For example: `100K → 35K → 15K → 5K`.

Compressed results must preserve Source References:

```json
{
  "summary": "The database connection pool was exhausted after a retry storm",
  "sources": [
    "tool://logs/request-92#L820-L1160",
    "file://config/db.yaml#L31-L58"
  ]
}
```

The model can call `expand(source)` to retrieve the original content again.

---

## 13. Memory Layers

`Conversation != Working Memory != Long-term Memory`.

```text
Session Event Log          Working Memory       Long-term Memory
├── Messages               ├── Current Goal     ├── User Preferences
├── Tool Calls             ├── Current Plan     ├── Workspace Knowledge
├── Decisions              ├── Findings         ├── Reusable Knowledge
├── Artifacts              ├── Pending Tasks    └── Historical Decisions
└── Observations           └── Temporary State
```

---

## 14. Capability Runtime

Capabilities must not belong to model providers.

```text
CapabilityRegistry
├── shell
├── filesystem
├── browser
├── computer
├── code_execution
├── git
├── web
├── mcp
├── database
└── custom
```

Therefore, Computer Use is not exclusive to Codex; it is a Runtime Capability.

---

## 15. Computer Use

```rust
trait ComputerProvider {
    async fn observe(&self, session: &ComputerSession) -> Result<ComputerObservation>;
    async fn execute(&self, action: ComputerAction) -> Result<ComputerObservation>;
}

enum ComputerAction {
    Click { x: u32, y: u32 },
    DoubleClick { x: u32, y: u32 },
    Type { text: String },
    Key { key: String },
    Scroll { dx: i32, dy: i32 },
    Drag { from: Point, to: Point },
}
```

Backends may be a Local Desktop Driver, Browser / Playwright, Remote Computer Driver, Vision-based Driver, or Third-party CUA Provider. The Agent depends only on the Capability, not a particular model.

---

## 16. Multi-Model Computer Use

One model need not do everything:

```text
Screenshot → Local Vision Model → Screen Semantic State → Reasoning Model
           → Computer Action
```

An Agent can combine a Control Model, Reasoning Model, Vision Model, Embedding Model, and Judge Model.

---

## 17. Tool Runtime

Unified abstractions: `ToolDefinition`, `ToolInvocation`, and `ToolResult`.

Responsibilities: Registry, Permission, Approval, Execution, Timeout, Retry, Redaction, and Audit. A model may only propose a `Tool Call Intent`; the Policy Engine decides whether execution is allowed.

---

## 18. Secret Broker

Secrets are a first-class core capability.

> A Secret never belongs in model context.

Claude, GPT, Qwen, and local llama.cpp models must not see Secrets directly by default.

### 18.1 SecretRef

Models use only logical references such as `payment.prod.credentials`, `github.company`, `postgres.prod.readonly`, or `secret://payment/prod/private-key`. Real values are never returned.

### 18.2 Late Secret Binding

```text
Model Planning → Tool Call → Policy Check → Secret Resolution → Execution
```

Bind Secrets only at the final trusted execution stage.

---

## 19. Payment Integration-Test Example

```json
{"tool":"payment.integration_test","arguments":{"environment":"production","merchant":"main","amount":0.01}}
```

```yaml
payment.integration_test:
  requires:
    secrets: [payment.main.api_key, payment.main.private_key]
```

```text
Agent → Tool Request → Policy Engine → Human Approval → Secret Broker
      → Local Signer → HTTP Request → Response Redaction → Model
```

The model never learns the `private_key`, `api_key`, or signature secret.

---

## 20. Secret Scope

Secrets must have capability boundaries.

```yaml
secret: payment.prod.private_key
allow:
  tools: [payment.query, payment.integration_test]
deny:
  tools: [shell, raw_http, browser]
environment: [production]
```

Even `shell: print PAYMENT_PRIVATE_KEY` cannot read it.

---

## 21. Secret Provider

Support OS Keychain, Local Encrypted Store, HashiCorp Vault, 1Password, Bitwarden, AWS Secrets Manager, Kubernetes Secrets, and Custom Providers. Do not place every Secret directly in the Agent process environment.

---

## 22. Secrets Need Not Be Readable

At higher security levels, forbid `fn get_secret() -> String`. Prefer `sign(secret_ref, payload)` or `execute_with_secret(tool, secret_ref)`.

```text
Agent → Request Signature → Secret Provider / HSM / Vault Transit → Signature
```

The Agent process never receives the private-key plaintext.

---

## 23. Output Redaction

Prevent leakage from outputs as well as inputs:

```text
Raw Tool Result → Secret Scanner → Redaction → Model Observation
```

```json
{"access_token":"[REDACTED]","order_id":"202609210001"}
```

---

## 24. Data Classification

Use `PUBLIC`, `INTERNAL`, `CONFIDENTIAL`, and `SECRET`.

```text
PUBLIC       → Remote allowed
INTERNAL     → According to organization policy
CONFIDENTIAL → Local preferred / trusted remote only
SECRET       → Never enter model context
```

---

## 25. Sync Server

The Sync Server is optional, not a hard Agent Runtime dependency. Local-first is the default. Support Offline Only, Official Hosted Sync, and Self-hosted Sync.

---

## 26. Sync Provider Abstraction

```rust
trait SyncProvider {
    async fn push(&self, objects: Vec<SyncObject>) -> Result<()>;
    async fn pull(&self, cursor: SyncCursor) -> Result<SyncBatch>;
}
```

Implementations: `NoopSync`, `LocalFolderSync`, `OfficialCloudSync`, and `SelfHostedSync`. The Agent must be fully usable without an account or server.

---

## 27. Sync Data Levels

### Level 1: Ordinary Configuration

Agent Profile, Working Style, UI Preferences, Model Preferences, Skill Settings, and Policy may be synchronized.

### Level 2: Workspace Metadata

Repo Fingerprint, Architecture Summary, Build Commands, Enabled Skills, and Project Conventions may be synchronized. Support `workspace.private = true`.

### Level 3: Sensitive Memory

Use Client-side Encryption.

### Level 4: Secrets

Default to Local Only. If synchronization is enabled, require Explicit Opt-in plus End-to-End Encryption. Prefer integrating a separate Secret Manager instead of turning the Sync Server into a password manager.

---

## 28. Sync Server Goals

The official Sync Server primarily synchronizes Identity, Profile, Preferences, Skills, Workspace Metadata, Memory Metadata, Session Index, and Device State. It is not the center of Agent execution.

---

## 29. Event-Driven Agent Kernel

Internally, the Kernel should use `Event Driven + State Machine`, not a simple infinite loop.

Events:

```text
SessionStarted
UserMessageReceived
TurnStarted
StepStarted
ContextPrepared
ModelSelected
ModelStarted
ModelDelta
ModelCompleted
ToolRequested
ToolApproved
ToolStarted
ToolCompleted
ObservationCreated
StepCompleted
TurnCompleted
SessionClosed
```

Benefits: Resume, Replay, Audit, Debug, Fork, Evaluation, and Checkpoint.

---

## 30. Agent State Machine

```text
IDLE → PREPARING_CONTEXT → SELECTING_MODEL → MODEL_RUNNING
                                             ├──→ COMPLETED
                                             └──→ TOOL_REQUIRED → TOOL_RUNNING
                                                                  ↓
                                                             OBSERVATION
                                                                  └──→ SELECTING_MODEL
```

---

## 31. Recommended Rust Module Layout

```text
agent-runtime/
├── crates/
│   ├── agent-core/       # identity, session, turn, step, event
│   ├── agent-kernel/     # runtime, state-machine, cancellation, retry, resume, sub-agent
│   ├── agent-model/      # provider, registry, capabilities, scheduler, policy
│   ├── agent-context/    # memory, retrieval, compression, budget
│   ├── agent-profile/
│   ├── agent-workspace/
│   ├── agent-skill/
│   ├── agent-capability/
│   ├── agent-tool/
│   ├── agent-secret/
│   ├── agent-sync/
│   ├── agent-mcp/
│   ├── agent-storage/
│   ├── agent-sandbox/
│   └── agent-telemetry/
├── providers/
│   ├── openai/
│   ├── anthropic/
│   ├── ollama/
│   └── llama-cpp/
├── apps/
│   ├── agent-cli/
│   ├── agent-server/
│   └── agent-desktop/
└── examples/
```

---

## 32. Recommended Rust Technology Stack

Core: Rust, Tokio, Serde, thiserror / anyhow, tracing, OpenTelemetry. Server: Axum, Tower, Reqwest. Plugins / Sandbox: Wasmtime, WASI. Storage: SQLite, PostgreSQL, RocksDB / Redb as needed. Models: OpenAI-compatible HTTP, Anthropic API, llama.cpp, Ollama, vLLM.

Rig Core may be reused as a Provider abstraction or reference, but the entire Agent Runtime should not be built directly on Rig Agent.

---

## 33. Plugin Model

Do not rely primarily on Rust `dylib` for third-party plugins. Prefer WASM Component, MCP, JSON-RPC, gRPC, and Process Plugin.

```text
Plugin
├── Capability Declaration
├── Permission
├── Filesystem Scope
├── Network Scope
├── Memory Limit
└── Execution Timeout
```

---

## 34. Sandbox

The Agent will ultimately need Process Sandbox, Docker Sandbox, WASM Sandbox, Remote Worker, and Workspace Isolation. Third-party Skills and Tools in particular must not receive full host permissions by default.

---

## 35. Observability

All modules uniformly produce Trace, Metric, Event, Cost, Token Usage, Model Route, Tool Call, Latency, Error, and Escalation data.

Core metrics:

```text
local_hit_rate
remote_escalation_rate
tokens_saved
remote_cost
local_latency
tool_success_rate
model_success_rate
context_compression_ratio
computer_use_success_rate
secret_policy_denied
```

---

## 36. Most Important Product Experiences

### 36.1 Switch Models Without Losing Continuity

`Local Qwen → Claude → GPT → Local Qwen` without re-explaining working habits, loading Skills, describing the Repository, or resupplying Agent Rules.

### 36.2 Zero Configuration for a New Repository

Running `agent .` automatically loads the Global Profile, detects the Repository, loads Repository Instructions, builds the Workspace Profile, matches Skills, and starts a Session.

### 36.3 Secrets Are Never Exposed to Models

```text
Agent wants action → Tool → SecretRef → Trusted Execution
```

Never `Secret → Prompt`.

### 36.4 Computer Use Is Decoupled from Models

Any model meeting the capability requirements can use the Computer Capability; desktop operation is not exclusive to one model provider.

---

## 37. Recommended Phase-One MVP

Implement only: Agent Kernel, Session/Event, Agent Profile, Workspace Runtime, Skill Registry, Model Provider, Model Scheduler, Local Control Model, Local Worker Model, Remote Fallback, Tool Registry, Shell, Filesystem, MCP, SecretRef, Basic Secret Broker, and CLI.

```text
Local Control: Qwen 2B–4B / llama.cpp
Local Worker:  Qwen 20B–30B
Remote:        Any 1–2 Frontier Providers
```

---

## 38. Phase Two

Add Context Compression, Long-term Memory, Sync Provider, Official Sync Server, Self-hosted Sync, Computer Use, Browser Use, WASM Plugin, Sandbox, Human Approval, Data Classification, and Advanced Secret Broker.

---

## 39. Phase Three

Add Multi-Agent, Remote Worker, Distributed Execution, Organization Policy, Enterprise Secret Manager, Agent Marketplace, Skill Marketplace, Evaluation System, Automatic Route Calibration, and Model Performance History.

---

## 40. Final Architectural Philosophy

```text
Agent owns:
    Identity
    Context
    Memory
    Workspace
    Skills
    Capabilities
    Tools
    Secrets
    State

Models provide:
    Compute
```

> **The Agent is the persistent subject; models are replaceable inference backends.**

Furthermore:

> **A local model is not a cheaper remote model; it is part of the Agent Control Plane.**

```text
             Persistent Agent
                   │
        ┌──────────┼──────────┐
        ↓          ↓          ↓
    Identity    Workspace   Memory
        │          │          │
        └──────────┼──────────┘
                   ↓
              Agent Kernel
                   │
              Model Scheduler
                   │
      ┌────────────┼─────────────┐
      ↓            ↓             ↓
Local Control  Local Worker   Remote Model
      │            │             │
      └────────────┼─────────────┘
                   ↓
              Capabilities
                   │
          ┌────────┼────────┐
          ↓        ↓        ↓
        Tools   Computer  Browser
          │
          ↓
     Secret Broker
          │
          ↓
      Execution
```

---

## 41. One-Sentence Project Positioning

> **A local-first, provider-neutral Agent Runtime where identity, context, capabilities and secrets belong to the Agent — models are replaceable compute providers.**

Chinese:

> **一个本地优先、模型供应商中立的通用 Agent Runtime：身份、上下文、能力和密钥属于 Agent，模型只是可替换的计算资源。**
