# Lyonbrown Phase-One MVP Implementation Plan

> Historical record: this document describes the phase-one monolithic CLI that
> was delivered before the C/S migration. Its `agent-cli` feature ownership,
> direct-Kernel commands, embedded-model defaults, and headless entry points are
> superseded and are not compatibility requirements. The current product
> boundary is defined by [Local gRPC Client/Server Architecture](../architecture/grpc-client-server.md):
> `agentd` is the sole Runtime/model owner and TUI/GUI are gRPC clients.
>
> Based on: [Lyonbrown Local-First Agent Runtime Architecture](../architecture/local-first-agent-runtime.md) (Draft v0.1)  
> Status: M0–M9 implemented and locally accepted; the M9 real-GGUF smoke test passed on Windows MSVC/CPU. Cross-platform sandboxing remains Phase 2.
> Platform: Windows 11 · rustc/cargo 1.98.1 · Ollama installed (qwen3.8-27b:q3-claude as Worker / translategemma:4b available as Control)

## Context

The repository has completed the M0 workspace skeleton and the foundational `agent-core` types. This plan continues by turning the phase-one MVP defined in architecture §37 into a testable, runnable Rust Agent Runtime:

Agent Kernel (event-driven + state machine) · Agent Profile · Workspace Runtime · Skill Registry · Model Provider + Scheduler (three model tiers, step-level scheduling, two-stage routing) · Tool Runtime (Shell/Filesystem) · MCP · SecretRef + basic Secret Broker · CLI

**Confirmed decisions:**

- Scope = the complete MVP in §37
- Local: Ollama Provider (default path) + embedded llama.cpp (optional feature, disabled by default, M9 stretch)
- Remote: Anthropic + OpenAI, both with streaming and tool calling
- All Kernel logic must first pass offline through MockProvider before connecting to real Providers

**Required abstraction seams (mandated by the architecture):**

- Model Provider Neutral: one `ModelProvider` trait; no hard-coded Provider enum
- Sessions belong to the Runtime: model switching does not lose working style, Workspace, Skills, Plan, or Tool state
- Secrets never enter model context: logical SecretRef + Late Binding + output Redaction
- Step-level Model Scheduling: models are Kernel-scheduled resources, not fixed per Session
- Event-driven Kernel: events + state machine; the event log is the sole source of truth for resume/replay/audit

---

## 1. Workspace Layout

Root: `D:\Projects\lyonbrown`; edition 2024; resolver 2; shared versions in workspace dependencies; `clippy::all` warn + `unsafe_code` forbid (the llamacpp crate may opt out later). M0 completed Git initialization and the workspace skeleton.

### MVP crate inventory (12 crates + 5 providers + 1 app, 18 packages total)

| Crate | Responsibility |
|---|---|
| `crates/agent-core` | Pure shared types, no logic: ids, errors, ContextItem, AgentEvent, tool types, ModelCapabilities, SecretRef |
| `crates/agent-kernel` | State machine, agent loop, event bus, session/working memory, retry/cancel |
| `crates/agent-model` | ModelProvider trait, registry, scheduler, policy, quality gate |
| `crates/agent-context` | Prompt Compiler (profile+workspace+skills→system prompt), context budget, token estimation; semantic/vector retrieval and compression are Phase 2 |
| `crates/agent-profile` | AgentProfile YAML loading/validation |
| `crates/agent-workspace` | Repository detection, WorkspaceProfile, AGENTS.md/CLAUDE.md parsing |
| `crates/agent-index` | Incremental workspace index via `ignore` + `notify`, SQLite FTS5 keyword search; exposed to the CLI as the read-only `search_workspace` tool |
| `crates/agent-skill` | Skill YAML, registry, trigger matching |
| `crates/agent-tool` | Tool trait, registry, executor, approval, timeout, redaction hooks + built-in Shell/FS tools |
| `crates/agent-secret` | SecretBroker (env/file/composite), late binding, output redaction |
| `crates/agent-mcp` | rmcp client bridge → integrated into Tool Registry |
| `crates/agent-storage` | SQLite (rusqlite bundled): session/event log, resume |
| `apps/agent-cli` | clap REPL, streaming renderer, approval prompts, configuration |
| `providers/provider-ollama` | Local Worker+Control (Ollama HTTP API) |
| `providers/provider-anthropic` | Remote (Messages API SSE) |
| `providers/provider-openai` | Remote (Chat Completions SSE) |
| `providers/provider-mock` | Scripted Mock Provider; all kernel/e2e tests run offline |
| `providers/provider-llamacpp` | Optional `llama-cpp` feature, disabled by default (M9 stretch) |

Trimmed from §31 as Phase 2/3 work (§38–39): `agent-capability` (folded into agent-tool for MVP), `agent-sync`, `agent-sandbox`, `agent-telemetry` (use tracing initially), `agent-server`, and `agent-desktop`. `agent-sandbox` now has a separate [cross-platform design](../architecture/cross-platform-sandbox.md): M3 Shell must identify itself as host/unsandboxed, while Phase 2 delivers WASI plus native OS backends.

### Dependency graph (strict DAG)

```text
agent-core (no internal dependencies)
  ├─► agent-profile / agent-workspace / agent-skill ─► agent-context
  ├─► agent-secret ─► agent-tool ─► agent-mcp
  ├─► agent-model ◄── providers/*
  └─► agent-storage
agent-index ─────────► agent-tool
  all of the above ─► agent-kernel ─► apps/agent-cli
```

- `agent-context` → core, profile, workspace, skill
- `agent-tool` → core, secret (the broker is required for late binding), index
- `agent-index` → rusqlite, ignore, notify, blake3
- `agent-mcp` → core, tool, rmcp
- `agent-kernel` → core, model, context, tool, secret, storage, profile, workspace, skill
- `agent-cli` → kernel + all providers (feature-gated: `ollama` on by default, `anthropic`/`openai` on, `llama-cpp` off)

### Modules by crate

- `agent-core/src/`: `lib.rs` `ids.rs` `error.rs` `context.rs` `event.rs` `tool.rs` `model.rs` `secret.rs`
- `agent-kernel/src/`: `lib.rs` `kernel.rs` `state.rs` `bus.rs` `loop.rs` `session.rs` `retry.rs`
- `agent-model/src/`: `lib.rs` `provider.rs` `registry.rs` `scheduler.rs` `policy.rs` `gate.rs`
- `agent-context/src/`: `lib.rs` `compiler.rs` `budget.rs` `tokens.rs`
- `agent-profile/src/`: `lib.rs` `profile.rs`
- `agent-workspace/src/`: `lib.rs` `detect.rs` `instructions.rs`
- `agent-index/src/`: `lib.rs` `index.rs` `scan.rs` `watcher.rs`
- `agent-skill/src/`: `lib.rs` `skill.rs` `registry.rs` `match.rs`
- `agent-tool/src/`: `lib.rs` `registry.rs` `execute.rs` `approval.rs` `redact.rs` `shell.rs` `fs.rs`
- `agent-secret/src/`: `lib.rs` `env_broker.rs` `file_broker.rs` `composite.rs` `redact.rs`
- `agent-mcp/src/`: `lib.rs` `bridge.rs` `config.rs`
- `agent-storage/src/`: `lib.rs` `db.rs` `session_store.rs`
- `agent-cli/src/`: `main.rs` `repl.rs` `render.rs` `approve.rs` `config.rs`
- `providers/*/src/`: `lib.rs` `client.rs` `stream.rs` (mock: `lib.rs` `scripted.rs`; llamacpp: `lib.rs`)

---

## 2. Key Types and Trait Signatures

All async traits use `#[async_trait]`; errors use thiserror and can all convert to `agent_core::AgentError`.

### agent-core

```rust
pub enum ContextItemKind { Message, ToolObservation, Document, Memory, Summary,
                           Artifact, Reference, WorkspaceState, Plan, Finding }
pub enum DataClassification { Public, Internal, Confidential, Secret }
pub struct ContextItem {
    pub id: Uuid, pub kind: ContextItemKind, pub content: String,
    pub priority: u8,                 // 0 = retain first
    pub token_estimate: u32,
    pub source: String,               // "user" | "tool://shell#<call-id>" | "file://AGENTS.md"
    pub timestamp: DateTime<Utc>, pub classification: DataClassification,
    pub expandable: bool,             // always false in MVP (compression is Phase 2)
}

// event.rs — all §29 events, serde tag="type"
pub enum AgentEvent {
    SessionStarted { session_id, ts }, UserMessageReceived { session_id, turn_id, text, ts },
    TurnStarted { session_id, turn_id, ts }, StepStarted { session_id, turn_id, step_id, ts },
    ContextPrepared { step_id, system_tokens, context_tokens, dropped, ts },
    ModelSelected { step_id, provider: String, model: String, reason: String, ts },
    ModelStarted { step_id, ts }, ModelDelta { step_id, delta: String },
    ModelCompleted { step_id, usage: TokenUsage, ts },
    ToolRequested { step_id, invocation: ToolInvocation, ts },
    ToolApproved { step_id, tool_call_id, by: String, ts }, ToolStarted { step_id, tool_call_id, ts },
    ToolCompleted { step_id, result: ToolResult, ts }, ObservationCreated { step_id, observation_id, ts },
    StepCompleted { step_id, outcome: StepOutcome, ts }, TurnCompleted { turn_id, answer: String, ts },
    SessionClosed { session_id, ts }, Error { session_id, message: String, ts },
}
pub struct StepOutcome { pub stop_reason, pub text, pub tool_calls: Vec<ToolCallId>, pub escalated: bool }

pub enum RiskLevel { Read, Write, Execute, Network } // drives approval policy
pub struct ToolDefinition {
    pub name: String, pub description: String, pub input_schema: serde_json::Value,
    pub source: ToolSource, pub risk: RiskLevel, pub requires_secrets: Vec<SecretRef>,
}
pub struct ToolInvocation { pub id: ToolCallId, pub name: String, pub arguments: serde_json::Value }
pub struct ToolResult { pub invocation_id: ToolCallId, pub ok: bool, pub output: String,
                        pub truncated: bool, pub duration_ms: u64, pub error: Option<String> }

pub enum ModelTier { Control, Worker, Frontier }
pub enum Locality { Local, Remote }
pub struct ModelCapabilities { pub provider: String, pub model: String, pub tier: ModelTier,
    pub locality: Locality, pub context_window: u32, pub max_output_tokens: u32,
    pub supports_tools: bool, pub supports_streaming: bool, pub supports_json: bool,
    pub supports_thinking: bool, pub cost_per_mtok_in: f64, pub cost_per_mtok_out: f64 }
pub struct TokenUsage { pub input: u32, pub output: u32, pub cache_read: u32 }
#[serde(transparent)]
pub struct SecretRef(pub String); // "env:ANTHROPIC_API_KEY" | "file:github"
```

### agent-model

```rust
pub struct ChatMessage { pub role: Role, pub content: String,
    pub tool_call_id: Option<ToolCallId>, pub tool_name: Option<String>,
    pub tool_calls: Vec<ToolInvocation> }
pub struct ModelRequest { pub model: String, pub system: Option<String>,
    pub messages: Vec<ChatMessage>, pub tools: Vec<ToolDefinition>, pub max_tokens: u32,
    pub temperature: Option<f32>, pub json_mode: bool, pub thinking: Option<bool>,
    pub stop: Vec<String> }
pub enum ModelEvent { Delta(String), ToolCall(ToolInvocation), Usage(TokenUsage),
                      Done { stop_reason: StopReason } }
pub enum StopReason { EndTurn, ToolUse, MaxTokens, Stop, Error(String) }
pub struct ModelStream { pub model: String,
    pub events: Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send>> }
#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    async fn list_models(&self) -> Result<Vec<ModelCapabilities>, ModelError>;
    async fn stream(&self, req: ModelRequest) -> Result<ModelStream, ModelError>;
}
```

### agent-tool / agent-secret

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> &ToolDefinition;
    async fn invoke(&self, ctx: &ToolContext, inv: &ToolInvocation) -> Result<ToolResult, ToolError>;
}
pub struct ToolContext { pub workspace_root: PathBuf, pub broker: Arc<dyn SecretBroker>,
                         pub cancel: CancellationToken, pub timeout: Duration }
pub enum ApprovalDecision { Allow, AllowSession, Deny }
pub trait ApprovalPolicy: Send + Sync {
    fn approve(&self, def: &ToolDefinition, inv: &ToolInvocation) -> ApprovalDecision;
}
#[async_trait]
pub trait SecretBroker: Send + Sync {
    /// Called only during execution (late binding §18.2), never on the prompt path.
    async fn resolve(&self, r: &SecretRef) -> Result<String, SecretError>;
    /// Replaces values resolved in this session with "[REDACTED]".
    fn redact(&self, text: &str) -> String;
}
```

MVP implementations: EnvSecretBroker, FileSecretBroker (`~/.agent/secrets/<name>`, mode 0600), CompositeBroker.

### agent-kernel — state machine + loop

```rust
pub enum KernelState { Idle, PreparingContext, SelectingModel, ModelRunning,
    ToolRequired, ToolRunning, Observation, Completed, Failed { error: String }, Cancelled }
impl KernelState {
    pub fn transition(self, ev: &AgentEvent) -> Result<KernelState, TransitionError>;
}
pub struct Kernel {
    pub session: Session, pub state: KernelState, pub bus: EventBus,
    pub scheduler: ModelScheduler, pub registry: Arc<ModelRegistry>,
    pub tools: Arc<ToolRegistry>, pub policy: Arc<dyn ApprovalPolicy>,
    pub broker: Arc<dyn SecretBroker>, pub storage: Option<Arc<SessionStore>>,
    pub limits: KernelLimits,
}
impl Kernel { pub async fn run_turn(&mut self, user_text: &str) -> Result<Vec<AgentEvent>, AgentError>; }
```

Loop: PreparingContext (compile system prompt, trim to budget) → SelectingModel (`scheduler.decide`, emit ModelSelected) → ModelRunning (consume ModelStream, emit ModelDelta) → on ToolUse, ToolRequired (approval) → ToolRunning (execute, late-bind secret, redact) → Observation (append ToolObservation ContextItem) → SelectingModel; on EndTurn, quality gate → Completed. Every transition emits an event; the bus log is the resume/replay source.

### agent-model — routing flow (§10/§11)

```rust
pub struct RoutingPolicy { pub prefer_local: bool, pub remote_escalation: bool,
    pub max_remote_cost_usd: f64, pub privacy: DataClassification }
pub struct SchedulerDecision { pub provider: String, pub model: String,
    pub phase: RoutePhase, pub reason: String }
impl ModelScheduler {
    pub fn decide(&self, ctx: &StepContext) -> SchedulerDecision {
        // 1. Static filters: privacy, capability, budget, locality.
        // 2. Complexity selects Control/Worker; escalated selects Frontier.
        // 3. Within a tier, choose the cheapest by (locality, cost).
    }
}
pub trait QualityGate: Send + Sync {
    fn evaluate(&self, outcome: &StepOutcome, caps: &ModelCapabilities) -> GateVerdict;
}
```

Default HeuristicGate escalates empty output, explicit error markers, or output too short for a high-complexity step. ControlModelGate asks the control model in JSON mode whether work is complete, format is valid, or uncertainty remains; it is feature-flagged in the MVP.

---

## 3. Third-Party Dependency Choices

**Required (M0–M2):**

| Crate | Notes | Used by |
|---|---|---|
| `tokio` 1.x | `rt-multi-thread, macros, process, io-util, time, signal` | All async code |
| `tokio-util` | `CancellationToken` | kernel, tool |
| `tokio-stream` / `futures` | ModelStream pipeline | model, providers |
| `async-trait` | Object-safe async traits | model, tool, secret |
| `serde` + `serde_json` | derive | Global |
| `serde_yml` | Maintained YAML (not unmaintained serde_yaml) | profile, skill |
| `toml` 1.1.6 | Direct TOML parsing, including MCP configuration | cli, mcp |
| `config-rs` (crate name `config`) 0.15.26 | Layered configuration; TOML/YAML/JSON/JSON5/RON/INI enabled | cli |
| `thiserror` 2.x / `anyhow` | Errors | Global |
| `tracing` + `tracing-subscriber` | env-filter; telemetry seam for Phase 2 | Global |
| `reqwest` 0.13.5 | `rustls, stream, json`; default features disabled (avoid OpenSSL on Windows) | providers |
| `clap` 4.x derive | CLI | cli |
| `rusqlite` | `bundled`; synchronous API through spawn_blocking | storage |
| `uuid` (v4) / `chrono` | IDs, timestamps | core |
| `dirs` | `~/.agent` | cli, storage, secret |
| `which` | Locate MCP server executables | mcp, workspace |
| `cap-std` 4.0.3 | Capability-relative filesystem access that contains symlinks/junctions within the workspace | tool |
| `wiremock` | **dev-dependency only** — offline provider HTTP tests | provider tests |

**Deferrable:**

| Crate | Flag | Timing |
|---|---|---|
| `rmcp` **3.4.0** (official MCP SDK; `client, transport-child-process`; verified latest stable on crates.io) | Needed only in M7; pin exact version and isolate all rmcp types inside agent-mcp | M7 |
| `tiktoken-rs` | Defer; begin with `chars/4` heuristic + provider TokenUsage | post-MVP |
| `ratatui` 0.30.2 (default Crossterm backend) | Full TUI; core exposes only AgentEvent and ApprovalPrompt | M8 |
| `llama-cpp-2` 0.1.156 | In-process embedded llama.cpp; optional `llama-cpp`, off by default | M9 stretch |
| `axum` / `opentelemetry` / `wasmtime` etc. | Defer | Phase 2/3 |

---

## 4. Milestones (M0–M9)

**M0 — Skeleton + agent-core (complete)** (0.5–1 day)

- Build: git init, workspace Cargo.toml (workspace deps, lints), 17 compiling package stubs; complete `agent-core`; rustfmt.toml, .gitignore
- Acceptance: clean `cargo build --workspace`; serde round-trip for every AgentEvent variant; ContextItem priority ordering; clean clippy
- Verify: `cargo test -p agent-core && cargo clippy --workspace -- -D warnings`

**M1 — Kernel + MockProvider (offline e2e, complete)** (2–3 days) — *core risk-reduction milestone*

- Build: agent-kernel state machine/transition table/EventBus/`run_turn`/memory/max-step guard/cancellation; agent-model trait/registry/gate; agent-tool registry/timed executor/approval; scripted provider-mock; echo + read_file test tools; §7 e2e
- Acceptance: offline user→mock tool call→observation→answer flow and full event assertion; exhaustive transitions; max-step loop guard; cancellation
- Verify: `cargo test -p agent-kernel --test e2e_mock` (no network)
- Note: M1 HeuristicGate handles only empty answers and explicit errors; escalation reselects the first current model. M2 implements complexity tiers, tier switching, and auditable routing reasons.

**M2 — Ollama Provider + local models (complete)** (2 days)

- Build: `/api/chat` NDJSON streaming, `tools`, `format:"json"`, worker `think:false`, usage from `done`; configure registry; scheduler filters/complexity; CLI v1 (`ask`, `chat`, streaming, provider/model flags, approve-all)
- Configuration: layered loading through `config-rs` 0.15.26, with TOML, YAML, JSON, JSON5, RON, and INI support; environment variables use the `LYONBROWN_` prefix. The fixed precedence is: built-in defaults < global configuration < workspace configuration < an explicitly specified configuration file < environment variables < CLI arguments
- Acceptance: translategemma streams “say hi”; tool steps route to qwen3.8-27b and never Control; scheduler tests select Control for simple input and Worker for code
- Verify: `cargo run -p agent-cli -- ask "1+1等于几" --provider ollama --model translategemma:4b`

**M3 — Tool Runtime: Shell + Filesystem + approval + redaction (complete)** (2–3 days)

- Build: shell via tokio process and `cmd.exe /c`, workspace cwd, 60s timeout, env hook, 32KB capped output; FS read/write/list through `cap-std` capability-relative access with early rejection of `..` and absolute paths; risk levels; interactive approval; deny-all; non-TTY fail-closed
- Acceptance: `..`, symlink/junction, and workspace-path replacement escapes rejected; the executor returns a timeout by its deadline and terminates the immediate shell (guaranteed descendant-tree cleanup belongs to the Phase 2 sandbox and is not claimed by M3); denial returns a failed ToolResult and loop continues; real Windows echo e2e
- Verify: `cargo test -p agent-tool` plus manual chat file listing

**M4 — Profile + Workspace + Skills + Prompt Compiler (complete)** (2–3 days)

- Build: profile YAML/default; manifest detection and canonical repository instructions with cache outside repo; Skill YAML/triggers; ordered prompt compiler; priority budget trimming retaining system/latest three user messages/latest tool observation; heuristic estimator; derive a real `needs_tools` signal from prompt/step intent, replacing M3's conservative temporary rule that any registered tool selects Worker
- Acceptance: zero-config Rust detection; deterministic five-part prompt fixture; quarkus trigger activates only on match; 200KB low priority trimmed to 8K while high priority remains
- Verify: `cargo test -p agent-context -p agent-workspace -p agent-skill -p agent-profile`

**M5 — Secrets (complete)** (1–2 days)

- Build: Env/File/Composite brokers; shell `{NAME:"secret:<ref>"}` resolution only at execution; session-wide resolved-value redaction
- Acceptance: child sees secret env, serialized events never contain plaintext, missing ref returns a safe error without panic/leak
- Verify: `cargo test -p agent-kernel --test secret_e2e`

**M6 — Remote Providers (Anthropic + OpenAI) + fallback (complete)** (3 days)

- Build: Anthropic Messages SSE/content blocks; OpenAI chat SSE/indexed delta accumulation; keys through SecretRef; one local-error→Frontier retry with fallback event reason
- Acceptance: offline wiremock event-sequence tests, auth/429 paths, mock fail-then-succeed fallback
- Verify: `cargo test -p provider-anthropic -p provider-openai` and `cargo test -p agent-kernel --test fallback` (offline); remote live smoke only with a key.

**M7 — MCP (complete)** (2 days)

- Build: exact-pinned rmcp 3.4.0, config and workspace override (MCP commands require `--trust-workspace-mcp`); spawn stdio servers; register namespaced tools; 60s timeout and common redaction
- Acceptance: local Rust fixture server, list/call round trip, kernel e2e observation, graceful server-crash error
- Verify: `cargo test -p agent-mcp` (offline)

**M8 — Storage, resume, finish (complete)** (2 days)

- Build: bundled SQLite/WAL/spawn_blocking; persist every event; resume/session listing; first Ctrl-C cancels cleanly, second forces. The full `ratatui` 0.30.2 + Crossterm TUI was delivered early after M4 (event, message, approval, and tool-result panes) while retaining the headless CLI
- Acceptance: resume after two interrupted steps preserves context; replay reconstructs equivalent working-memory content (item IDs/timestamps may differ); workspace green
- Verify: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`

**M9 — llama.cpp (stretch; real-model local acceptance complete)** (1–2 days)

- Build: `provider-llamacpp` embeds llama.cpp **in process** through `llama-cpp-2` 0.1.156. The Agent loads GGUF itself, owns backend/model/context lifetimes, and normalizes streamed tokens into `ModelEvent`; there is no HTTP daemon or external inference process. Isolate native compilation behind the optional `llama-cpp` feature, default to CPU, add CUDA/Vulkan later by feature, load a model once, and serialize inference until a context pool is justified.
- Acceptance: feature build and one small-GGUF completion. Drop M9 if the prebuilt artifact fails; Ollama already covers local inference and MVP does not depend on it.
- Verify: `cargo test -p provider-llamacpp --features llama-cpp`; with `LYONBROWN_GGUF_MODEL` set, run `cargo test -p provider-llamacpp --features llama-cpp live_completion_smoke_test -- --ignored`.

---

## 5. Key Design Decisions

**D1. Event bus = in-process `tokio::sync::broadcast` + append-only log; no actor framework or external broker.** `EventBus { tx, log }`; the Kernel loop is sequential async. Every transition emits an event, the log is the sole resume/replay/audit source (SQLite in M8), and CLI/telemetry subscribe to broadcast. Actor-per-session adds no value for a single-user CLI and makes cancellation/retry harder to test; a sequential loop plus event log delivers §29's benefits in roughly 100 lines.

**D2. Agent loop ↔ scheduler: Kernel owns the loop; scheduler is a pure decision function called once per step.** Kernel constructs StepContext before each step. `scheduler.decide()` is synchronously testable without a runtime. After local EndTurn, the quality gate runs; escalation re-enters SelectingModel with `escalated=true`, with exactly one escalation per step. This realizes §11 as data flowing through the existing state machine.

**D3. CLI approval UX: risk tiers, four keys + flags.** Read auto-allows. Write/Execute/Network show tool, formatted JSON arguments, and secret-reference names, then `[y]es / [n]o / [a]lways-this-session / [d]eny-all-rest`. Flags are approve-all/deny-all; non-TTY fails closed for high risk. Denial produces `ToolResult{ok:false,error:"denied by user"}` and the loop continues.

**D4. SecretRef late binding and shell.** Input is `{ command, env?: { [NAME]: "secret:<ref>" } }`. During ToolRunning, resolved pairs are injected only into the child environment. Defense in depth registers values in the redaction set, redacts output before ToolCompleted, and tests every serialized event for plaintext. Prompt assembly never touches the broker, making Secret entry into ModelRequest structurally impossible.

**D5. Context assembly order.** System prompt: AgentProfile → model capabilities → WorkspaceProfile → canonical repository instructions → matched Skills → tool guidance only for models without native tools. ContextItems sort by `(priority asc, timestamp asc)` and trim to budget while retaining system, latest three user messages, and latest tool observation. Determinism makes prompt diffs/debugging controllable and follows §7.1 precedence.

**D6. MCP tools are first-class Tool Registry entries.** `agent-mcp` is a bridge, not a parallel execution path. It registers `mcp.<server>.<tool>` entries in the same registry; Kernel, scheduler, approval, redaction, and audit treat them like built-ins. No MCP-specific code exists below agent-mcp, making M7 a removable leaf milestone.

**D7. The TUI is an optional AgentEvent/ApprovalPrompt adapter, not part of Kernel.** The full terminal UI uses `ratatui` with its default Crossterm backend, subscribes to the same event stream as the current CLI, and submits decisions through the same approval interface. Pipes, CI, and screen-reader workflows retain the plain-text/headless path and non-TTY fail-closed behavior.

**D8. provider-llamacpp is an embedded inference runtime, not an external-service adapter.** With the feature enabled, the process loads GGUF and samples directly while the provider maps agent-model requests to llama.cpp contexts and emits normalized events. This gives the Agent its own local-model execution capability while Ollama remains the lower-friction default local path.

---

## 6. Risks

| # | Risk | Probability/impact | Mitigation |
|---|---|---|---|
| R1 | Embedded llama.cpp on Windows MSVC: native C/C++ compilation plus CRT/link and GPU-backend combinations | Medium/high if forced | Isolate `llama-cpp-2` in provider-llamacpp behind an off-by-default feature; start CPU-only and validate it separately in CI; M9 remains removable because Ollama covers the default local path |
| R2 | MCP Rust SDK maturity and breaking 1.x→2.x→3.x changes | Medium/medium | Exact pin; isolate rmcp types; local Rust fixture; CLI feature gate |
| R3 | Provider protocol differences can silently break tool calls | High/medium | Normalize per provider; precise offline wiremock sequences; disable qwen thinking; common JSON Schema |
| R4 | Windows quoting, cwd/inheritance, junction/8.3 paths, line endings, GBK/UTF-8 | Medium/medium | Explicit cwd and one argument to cmd; M3 uses cap-std 4.0.3 handle-relative access plus early `..`/absolute-path rejection, with Windows junction and workspace-replacement regressions; shell output is byte-read/lossy UTF-8 while FS reads require UTF-8; retain a plain approval UI path |
| R5 | Control model lacks tools | Certain/low if respected | Discover capabilities; filter tool steps; assert before request; test Worker yes/Control no in M2 |
| R6 | 27B Q3 Worker latency | Medium/low | 300s model vs 60s tool timeout; stream progress; cap at 20 steps |

---

## 7. End-to-End Verification

### M1 acceptance test (offline)

File: `crates/agent-kernel/tests/e2e_mock.rs` — **no network, Ollama, or API key**: `cargo test -p agent-kernel --test e2e_mock`.

Scenario: the user asks what echo says with “hello”; mock first emits a tool call, observes its result, then emits the final answer.

```rust
#[tokio::test]
async fn user_message_to_tool_call_to_answer() {
    let tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool)); // returns args.text
    let providers = ModelRegistry::new();
    providers.register(Arc::new(MockProvider::scripted(vec![
        ScriptedStep::tool_call("echo", json!({"text": "hello"})),
        ScriptedStep::text("The echo tool returned: hello."),
    ])));
    let mut kernel = Kernel::builder()
        .providers(providers).tools(tools)
        .approval(AllowAllApproval).broker(NoopBroker)
        .limits(KernelLimits { max_steps_per_turn: 20, ..Default::default() })
        .build();
    let events = kernel.run_turn("What does the echo tool say with 'hello'?").await.unwrap();
    let kinds: Vec<_> = events.iter().map(|e| e.kind()).collect();
    assert_eq!(kinds, vec![
        SessionStarted, UserMessageReceived, TurnStarted,
        StepStarted, ContextPrepared, ModelSelected, ModelStarted,
        ModelDelta, ModelCompleted,
        ToolRequested, ToolApproved, ToolStarted, ToolCompleted,
        ObservationCreated, StepCompleted,
        StepStarted, ContextPrepared, ModelSelected, ModelStarted,
        ModelDelta, ModelCompleted, StepCompleted, TurnCompleted,
    ]);
    let obs = events.iter().find_map(|e| e.tool_result()).unwrap();
    assert!(obs.ok && obs.output == "hello");
    let req2 = kernel.last_requests().last().unwrap();
    assert!(req2.messages.iter().any(|m| m.role == Role::Tool && m.content == "hello"));
    let answer = events.iter().find_map(|e| e.turn_answer()).unwrap();
    assert!(answer.contains("hello"));
}
```

This test proves §37's core claim offline: **user message → model → tool call → observation → model → final answer**. Its event log is complete enough for M8 resume/replay and M5 secret scanning.

### Final global verification (after M8)

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
mdbook build docs/zh
mdbook build docs/en
```

Ollama and remote-provider live smoke tests require the corresponding service, model, or API key; offline acceptance does not. Every `.rs` file must also meet the 300-line limit in `AGENTS.md`.

---

## Key Files

- `crates/agent-kernel/src/kernel/` — agent loop, context preparation, fallback, and tool execution; `kernel.rs` holds Kernel state and public entry points.
- `crates/agent-model/src/provider.rs` — provider-neutral seam for ModelProvider/Request/Stream/Event
- `crates/agent-core/src/event.rs` — AgentEvent contract for resume, audit, redaction scanning, and e2e assertions
- `crates/agent-tool/src/shell.rs` — highest-risk built-in: path jail, timeout, late-bound secret env injection
- `crates/agent-kernel/tests/e2e_mock.rs` — offline acceptance gate for M1 and the full MVP
