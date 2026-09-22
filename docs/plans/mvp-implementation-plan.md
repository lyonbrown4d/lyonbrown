# Lyonbrown 第一阶段 MVP 实施计划

> 依据：[Lyonbrown Local-First Agent Runtime 架构设计](../architecture/local-first-agent-runtime.md)（Draft v0.1）
> 状态：M0 已完成；下一阶段为 M1
> 平台：Windows 11 · rustc/cargo 1.98.1 · Ollama 已装（qwen3.8-27b:q3-claude 作 Worker / translategemma:4b 可作 Control）

## Context（背景）

仓库已经完成 M0 workspace 骨架与 `agent-core` 基础类型。本计划继续把架构文档 §37 定义的第一阶段 MVP 落地为可测试、可运行的 Rust Agent Runtime：

Agent Kernel（事件驱动 + 状态机）· Agent Profile · Workspace Runtime · Skill Registry · Model Provider + Scheduler（三层模型、Step 级调度、两阶段路由）· Tool Runtime（Shell/Filesystem）· MCP · SecretRef + 基础 Secret Broker · CLI

**已确认决策：**
- 范围 = §37 完整 MVP
- 本地：Ollama Provider（默认路径）+ embedded llama.cpp（可选 feature，默认关闭，M9 stretch）
- 远程：Anthropic + OpenAI，均流式 + tool calling
- Kernel 全部逻辑必须先经 MockProvider 离线跑通，再接真实 Provider

**必须保留的抽象接缝（文档强制）：**
- Model Provider Neutral：统一 `ModelProvider` trait，禁止写死 Provider enum
- Session 属于 Runtime：模型切换不丢失工作风格/Workspace/Skill/Plan/Tool 状态
- Secret 永不进入模型上下文：SecretRef 逻辑引用 + Late Binding + 输出 Redaction
- Step-level Model Scheduling：模型是 Kernel 可调度资源，非 Session 固定
- Event-driven Kernel：事件 + 状态机；事件日志是 resume/replay/audit 的唯一事实源

---

## 1. Workspace 布局

根目录 `D:\Projects\lyonbrown`，edition 2024，resolver 2，workspace deps 表统一版本，`clippy::all` warn + `unsafe_code` forbid（llamacpp crate 后续单独放开）。M0 已完成 Git 初始化和 workspace 骨架搭建。

### MVP crate 清单（11 crates + 5 providers + 1 app，共 17 个包）

| Crate | 职责 |
|---|---|
| `crates/agent-core` | 纯共享类型、无逻辑：ids、errors、ContextItem、AgentEvent、tool 类型、ModelCapabilities、SecretRef |
| `crates/agent-kernel` | 状态机、agent loop、事件总线、session/working memory、retry/cancel |
| `crates/agent-model` | ModelProvider trait、registry、scheduler、policy、quality gate |
| `crates/agent-context` | Prompt Compiler（profile+workspace+skills→system prompt）、context budget、token 估算（retrieval/compression 属 Phase 2） |
| `crates/agent-profile` | AgentProfile YAML 加载/校验 |
| `crates/agent-workspace` | 仓库检测、WorkspaceProfile、AGENTS.md/CLAUDE.md 解析 |
| `crates/agent-skill` | Skill YAML、registry、trigger 匹配 |
| `crates/agent-tool` | Tool trait、registry、执行器、审批、超时、redaction 钩子 + 内置 Shell/FS 工具 |
| `crates/agent-secret` | SecretBroker（env/file/composite）、延迟绑定、输出 redaction |
| `crates/agent-mcp` | rmcp client 桥接 → 并入 Tool Registry |
| `crates/agent-storage` | SQLite（rusqlite bundled）：session/event 日志、resume |
| `apps/agent-cli` | clap REPL、流式渲染、审批提示、配置 |
| `providers/provider-ollama` | 本地 Worker+Control（Ollama HTTP API） |
| `providers/provider-anthropic` | 远程（Messages API SSE） |
| `providers/provider-openai` | 远程（Chat Completions SSE） |
| `providers/provider-mock` | 脚本化 Mock Provider，全部 kernel/e2e 测试离线跑 |
| `providers/provider-llamacpp` | 可选 feature `llama-cpp`、默认关闭（M9 stretch） |

从 §31 中裁剪（属 Phase 2/3，见 §38-39）：`agent-capability`（MVP 并入 agent-tool）、`agent-sync`、`agent-sandbox`、`agent-telemetry`（先用 tracing）、`agent-server`、`agent-desktop`。

### 依赖图（严格 DAG）

```
agent-core（无内部依赖）
  ├─► agent-profile / agent-workspace / agent-skill ─► agent-context
  ├─► agent-secret ─► agent-tool ─► agent-mcp
  ├─► agent-model ◄── providers/*
  └─► agent-storage
  以上全部 ─► agent-kernel ─► apps/agent-cli
```

- `agent-context` → core, profile, workspace, skill
- `agent-tool` → core, secret（延迟绑定需要 broker）
- `agent-mcp` → core, tool, rmcp
- `agent-kernel` → core, model, context, tool, secret, storage, profile, workspace, skill
- `agent-cli` → kernel + 全部 providers（feature-gated：`ollama` 默认开、`anthropic`/`openai` 开、`llama-cpp` 关）

### 各 crate 模块

- `agent-core/src/`：`lib.rs` `ids.rs` `error.rs` `context.rs` `event.rs` `tool.rs` `model.rs` `secret.rs`
- `agent-kernel/src/`：`lib.rs` `kernel.rs` `state.rs` `bus.rs` `loop.rs` `session.rs` `retry.rs`
- `agent-model/src/`：`lib.rs` `provider.rs` `registry.rs` `scheduler.rs` `policy.rs` `gate.rs`
- `agent-context/src/`：`lib.rs` `compiler.rs` `budget.rs` `tokens.rs`
- `agent-profile/src/`：`lib.rs` `profile.rs`
- `agent-workspace/src/`：`lib.rs` `detect.rs` `instructions.rs`
- `agent-skill/src/`：`lib.rs` `skill.rs` `registry.rs` `match.rs`
- `agent-tool/src/`：`lib.rs` `registry.rs` `execute.rs` `approval.rs` `redact.rs` `shell.rs` `fs.rs`
- `agent-secret/src/`：`lib.rs` `env_broker.rs` `file_broker.rs` `composite.rs` `redact.rs`
- `agent-mcp/src/`：`lib.rs` `bridge.rs` `config.rs`
- `agent-storage/src/`：`lib.rs` `db.rs` `session_store.rs`
- `agent-cli/src/`：`main.rs` `repl.rs` `render.rs` `approve.rs` `config.rs`
- `providers/*/src/`：`lib.rs` `client.rs` `stream.rs`（mock：`lib.rs` `scripted.rs`；llamacpp：`lib.rs`）

---

## 2. 关键类型与 Trait 签名

异步 trait 统一用 `#[async_trait]`；错误用 thiserror，均可转 `agent_core::AgentError`。

### agent-core

```rust
// context.rs
pub enum ContextItemKind { Message, ToolObservation, Document, Memory, Summary,
                           Artifact, Reference, WorkspaceState, Plan, Finding }
pub enum DataClassification { Public, Internal, Confidential, Secret }

pub struct ContextItem {
    pub id: Uuid, pub kind: ContextItemKind, pub content: String,
    pub priority: u8,                 // 0 = 最先保留
    pub token_estimate: u32,
    pub source: String,               // "user" | "tool://shell#<call-id>" | "file://AGENTS.md"
    pub timestamp: DateTime<Utc>,
    pub classification: DataClassification,
    pub expandable: bool,             // MVP 恒 false（压缩是 Phase 2）
}

// event.rs — §29 全量事件，serde tag="type"
pub enum AgentEvent {
    SessionStarted { session_id, ts },
    UserMessageReceived { session_id, turn_id, text, ts },
    TurnStarted { session_id, turn_id, ts },
    StepStarted { session_id, turn_id, step_id, ts },
    ContextPrepared { step_id, system_tokens, context_tokens, dropped, ts },
    ModelSelected { step_id, provider: String, model: String, reason: String, ts },
    ModelStarted { step_id, ts },
    ModelDelta { step_id, delta: String },
    ModelCompleted { step_id, usage: TokenUsage, ts },
    ToolRequested { step_id, invocation: ToolInvocation, ts },
    ToolApproved { step_id, tool_call_id, by: String, ts },
    ToolStarted { step_id, tool_call_id, ts },
    ToolCompleted { step_id, result: ToolResult, ts },
    ObservationCreated { step_id, observation_id, ts },
    StepCompleted { step_id, outcome: StepOutcome, ts },
    TurnCompleted { turn_id, answer: String, ts },
    SessionClosed { session_id, ts },
    Error { session_id, message: String, ts },
}
pub struct StepOutcome { pub stop_reason, pub text, pub tool_calls: Vec<ToolCallId>, pub escalated: bool }

// tool.rs
pub enum RiskLevel { Read, Write, Execute, Network }   // 驱动审批策略
pub struct ToolDefinition {
    pub name: String, pub description: String,
    pub input_schema: serde_json::Value,  // JSON Schema（三种 wire 格式的公分母）
    pub source: ToolSource,               // BuiltIn | Mcp { server: String }
    pub risk: RiskLevel,
    pub requires_secrets: Vec<SecretRef>, // 仅声明，执行时才 resolve
}
pub struct ToolInvocation { pub id: ToolCallId, pub name: String, pub arguments: serde_json::Value }
pub struct ToolResult { pub invocation_id: ToolCallId, pub ok: bool, pub output: String,
                        pub truncated: bool, pub duration_ms: u64, pub error: Option<String> }
// ToolResult.output 构造前必须已 redact

// model.rs
pub enum ModelTier { Control, Worker, Frontier }
pub enum Locality { Local, Remote }
pub struct ModelCapabilities { pub provider: String, pub model: String, pub tier: ModelTier,
    pub locality: Locality, pub context_window: u32, pub max_output_tokens: u32,
    pub supports_tools: bool, pub supports_streaming: bool, pub supports_json: bool,
    pub supports_thinking: bool, pub cost_per_mtok_in: f64, pub cost_per_mtok_out: f64 }
pub struct TokenUsage { pub input: u32, pub output: u32, pub cache_read: u32 }

// secret.rs
#[serde(transparent)]
pub struct SecretRef(pub String); // "env:ANTHROPIC_API_KEY" | "file:github"（keychain = Phase 2）
```

### agent-model

```rust
pub struct ChatMessage { pub role: Role /* System|User|Assistant|Tool */, pub content: String,
    pub tool_call_id: Option<ToolCallId>, pub tool_name: Option<String>,
    pub tool_calls: Vec<ToolInvocation> }

pub struct ModelRequest { pub model: String, pub system: Option<String>,
    pub messages: Vec<ChatMessage>, pub tools: Vec<ToolDefinition>, pub max_tokens: u32,
    pub temperature: Option<f32>, pub json_mode: bool,
    pub thinking: Option<bool>,   // qwen3.8 worker MVP 传 Some(false)
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
// agent-tool
#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> &ToolDefinition;
    async fn invoke(&self, ctx: &ToolContext, inv: &ToolInvocation) -> Result<ToolResult, ToolError>;
}
pub struct ToolContext { pub workspace_root: PathBuf, pub broker: Arc<dyn SecretBroker>,
                         pub cancel: CancellationToken, pub timeout: Duration }
pub enum ApprovalDecision { Allow, AllowSession, Deny }
pub trait ApprovalPolicy: Send + Sync {   // CLI 交互式实现；测试用 AllowAll/DenyRisky
    fn approve(&self, def: &ToolDefinition, inv: &ToolInvocation) -> ApprovalDecision;
}

// agent-secret
#[async_trait]
pub trait SecretBroker: Send + Sync {
    /// 仅执行期调用（late binding §18.2），永不出现在 prompt 路径
    async fn resolve(&self, r: &SecretRef) -> Result<String, SecretError>;
    /// 给 shell 用：注入子进程 env，值永不进入模型上下文
    async fn env_for(&self, refs: &[SecretRef]) -> Result<Vec<(String, String)>, SecretError>;
    /// 扫描工具输出，把本会话已解析过的值替换为 "[REDACTED]"
    fn redact(&self, text: &str) -> String;
}
// MVP 实现：EnvSecretBroker、FileSecretBroker（~/.agent/secrets/<name>，0600）、CompositeBroker
```

### agent-kernel — 状态机 + 循环

```rust
// state.rs — §30
pub enum KernelState { Idle, PreparingContext, SelectingModel, ModelRunning,
    ToolRequired, ToolRunning, Observation, Completed, Failed { error: String }, Cancelled }
impl KernelState {
    pub fn transition(self, ev: &AgentEvent) -> Result<KernelState, TransitionError>; // 纯转换表，穷举单测
}

pub struct Kernel {
    pub session: Session,              // working memory: Vec<ContextItem>, goal, plan
    pub state: KernelState,
    pub bus: EventBus,                 // broadcast + 内存日志
    pub scheduler: ModelScheduler,
    pub registry: Arc<ModelRegistry>,
    pub tools: Arc<ToolRegistry>,
    pub policy: Arc<dyn ApprovalPolicy>,
    pub broker: Arc<dyn SecretBroker>,
    pub storage: Option<Arc<SessionStore>>,
    pub limits: KernelLimits,          // max_steps_per_turn: 20, max_escalations: 1
}
impl Kernel { pub async fn run_turn(&mut self, user_text: &str) -> Result<Vec<AgentEvent>, AgentError>; }
// 循环：PreparingContext（编译 system prompt、budget 裁剪）→ SelectingModel（scheduler.decide，
// 发 ModelSelected）→ ModelRunning（消费 ModelStream，发 ModelDelta）→ StopReason::ToolUse 时
// ToolRequired（审批）→ ToolRunning（执行、secret 延迟绑定、redact）→ Observation（追加
// ToolObservation ContextItem）→ 回 SelectingModel；EndTurn 时 quality gate → Completed。
// 每次转换都向总线发事件；总线日志即 resume/replay 源。
```

### agent-model — 调度决策流（§10/§11）

```rust
pub struct RoutingPolicy { pub prefer_local: bool, pub remote_escalation: bool,
    pub max_remote_cost_usd: f64, pub privacy: DataClassification }
pub struct SchedulerDecision { pub provider: String, pub model: String,
    pub phase: RoutePhase /* Pre | Post */, pub reason: String }
impl ModelScheduler {
    pub fn decide(&self, ctx: &StepContext) -> SchedulerDecision {
        // 1. 静态过滤：privacy（Secret/Confidential + prefer_local ⇒ 仅本地）、
        //    capability（needs_tools ⇒ 仅 supports_tools）、budget、locality
        // 2. 复杂度：ctx.complexity（ControlModelGate 输出或启发式：消息长度、代码标记、
        //    前序 step 数）→ tier：low⇒Control（若 !needs_tools）| medium/high⇒Worker；
        //    ctx.escalated⇒Frontier
        // 3. tier 内按 (locality, cost) 选最便宜
    }
}
pub trait QualityGate: Send + Sync {
    fn evaluate(&self, outcome: &StepOutcome, caps: &ModelCapabilities) -> GateVerdict; // Pass | Escalate(String)
}
// HeuristicGate（默认）：输出为空/错误标记/高复杂度 step 输出过短 ⇒ 升级。
// ControlModelGate：让 control 模型（json_mode）判"完成？格式 OK？不确定？" — MVP 藏 flag 后。
```

---

## 3. 第三方依赖选型

**必须（M0–M2）：**

| Crate | 备注 | 使用者 |
|---|---|---|
| `tokio` 1.x | `rt-multi-thread, macros, process, io-util, time, signal` | 全部异步 |
| `tokio-util` | `CancellationToken` | kernel, tool |
| `tokio-stream` / `futures` | ModelStream 管道 | model, providers |
| `async-trait` | 对象安全异步 trait | model, tool, secret |
| `serde` + `serde_json` | derive | 全局 |
| `serde_yml` | 维护中的 YAML（非停维护的 serde_yaml） | profile, skill |
| `toml` | 配置文件 | cli, mcp |
| `thiserror` 2.x / `anyhow` | 错误 | 全局 |
| `tracing` + `tracing-subscriber` | env-filter（Phase 2 的 telemetry 接缝） | 全局 |
| `reqwest` 0.12 | `rustls-tls, stream, json`，**不用默认 TLS**（Windows 避开 OpenSSL） | providers |
| `clap` 4.x derive | CLI | cli |
| `rusqlite` | `bundled`（自带 SQLite C，无系统依赖；同步 API 走 spawn_blocking） | storage |
| `uuid` (v4) / `chrono` | id、时间戳 | core |
| `dirs` | `~/.agent` | cli, storage, secret |
| `which` | 定位 MCP server 可执行文件 | mcp, workspace |
| `wiremock` | **仅 dev-dependency** — provider 离线 HTTP 测试 | providers (tests) |

**可延后：**

| Crate | 标记 | 时机 |
|---|---|---|
| `rmcp` **3.4.0**（官方 MCP SDK，`client, transport-child-process`，已核实为 crates.io 最新稳定版） | M7 才需要——**pin 精确版本**，所有 rmcp 类型隔离在 agent-mcp 内 | M7 |
| `tiktoken-rs` | 延后——先用 `chars/4` 启发式 + provider 回报的 TokenUsage，不够准再加 feature | post-MVP |
| `llama-cpp-rs`（eugenehp，预编译 MSVC artifact） | 可选 feature `llama-cpp`，默认关 | M9 stretch |
| `axum` / `opentelemetry` / `wasmtime` 等 | 延后 | Phase 2/3 |

---

## 4. 里程碑（M0–M9）

**M0 — Skeleton + agent-core（已完成）**（0.5–1 天）
- 构建：git init、workspace Cargo.toml（workspace deps、lint）、17 个包 stub 全部可编译；`agent-core` 完整（ids/errors/ContextItem/AgentEvent/tool 类型/ModelCapabilities/SecretRef）；rustfmt.toml、.gitignore
- 验收：`cargo build --workspace` 干净；每个 AgentEvent 变体 serde round-trip 测试；ContextItem priority 排序测试；clippy 干净
- 验证：`cargo test -p agent-core && cargo clippy --workspace -- -D warnings`

**M1 — Kernel + MockProvider（离线 e2e，下一阶段）**（2–3 天）——*去风险核心里程碑*
- 构建：agent-kernel（状态机 + 转换表、EventBus、`Kernel::run_turn`、session/working memory、max-steps 防护、cancel token）；agent-model（trait、ModelRegistry、HeuristicGate）；agent-tool（registry、带超时执行器、ApprovalPolicy + AllowAll）；provider-mock（脚本化 step）；内置 echo + read_file 测试工具；§7 的 e2e 测试
- 验收：离线 e2e 通过（用户消息→mock 工具调用→观察→最终回答，全事件序列断言）；状态机转换表穷举测试；max-steps 循环防护测试；cancel 测试
- 验证：`cargo test -p agent-kernel --test e2e_mock`（无网络）

**M2 — Ollama Provider + 本地模型**（2 天）
- 构建：provider-ollama（`/api/chat` NDJSON 流式、`tools` 参数、`format:"json"`、qwen worker `think:false`、`done` chunk 取 usage）；从配置装配 ModelRegistry；ModelScheduler 静态过滤 + 启发式复杂度；CLI v1（`agent ask`、`agent chat` REPL、流式渲染、`--provider/--model`、`--approve-all`）
- 验收：`agent ask "say hi"` 从 translategemma:4b 流出；工具 step 路由到 qwen3.8-27b（control 模型永不接工具 step——用 fake registry 断言 ModelSelected 事件）；scheduler 单测：简单输入选 Control、代码输入选 Worker
- 验证：`cargo run -p agent-cli -- ask "1+1等于几" --provider ollama --model translategemma:4b`（Ollama 已在 :11434）

**M3 — Tool Runtime：Shell + Filesystem + 审批 + redaction**（2–3 天）
- 构建：shell 工具（tokio::process、默认 `cmd.exe /c`、cwd=workspace root、60s 超时、env 注入钩子、输出上限 32KB + 截断标志）；fs 工具（read/write/list——`canonicalize` + 前缀检查路径监狱，拒绝 `..` 逃逸）；风险级别（shell=Execute、fs.write=Write、read/list=Read）；CLI 审批提示（y/n/a/d），Write/Execute 才问，Read 自动放行；`--deny-all`；非 TTY 默认拒绝高风险（fail-closed）
- 验收：单测：`../etc/passwd` 逃逸被拒；超时杀死挂起命令；审批 Deny 产出 `ToolResult{ok:false}` 且循环继续；e2e：模型调 shell → Windows 上真实 echo 执行
- 验证：`cargo test -p agent-tool` + 手动 `cargo run -p agent-cli -- chat` 让它列文件

**M4 — Profile + Workspace + Skills + Prompt Compiler**（2–3 天）
- 构建：agent-profile（§5 YAML，`~/.agent/profiles/<id>.yaml`，内置默认 profile）；agent-workspace（manifest 检测、AGENTS.md/CLAUDE.md/README/CONTRIBUTING → canonical instructions、`~/.agent/workspaces/<hash>/` 缓存、不污染仓库）；agent-skill（`~/.agent/skills/` YAML + trigger 匹配）；agent-context 编译器：system prompt = profile → 模型能力说明 → workspace profile → 仓库指令 → 匹配 skills → 工具使用说明；budget 裁剪（优先级序，system + 最近 3 条用户消息 + 最近工具观察永不丢）；TokenEstimator 启发式
- 验收：`agent .` 在全新 cargo 仓库零配置检测出 Rust workspace；编译出的 system prompt 五段齐全且顺序固定（fixture 仓库断言）；trigger "quarkus" 的 skill 仅在词出现时激活；budget 测试：200KB 低优先级被裁到 8K 预算内、高优先级保留
- 验证：`cargo test -p agent-context -p agent-workspace -p agent-skill -p agent-profile`

**M5 — Secrets**（1–2 天）
- 构建：agent-secret（EnvSecretBroker、FileSecretBroker 读 `~/.agent/secrets/<name>`（裸值或 KEY=VALUE，创建时 0600）、CompositeBroker）；shell 工具 env 参数约定 `{NAME: "secret:<ref>"}` 仅执行期解析；redaction：会话级已解析值集合，扫出所有 ToolResult.output 与观察
- 验收：**安全测试**——脚本 mock 请求 shell 带 `secret:test_value` 引用；子进程看到 env（测试用 `echo` 断言）；断言序列化事件日志中任何 AgentEvent 变体都不含 secret 明文（全变体扫描）；引用缺失 ⇒ `ToolResult{ok:false, error:"secret not found"}`，不 panic、不泄漏
- 验证：`cargo test -p agent-secret -p agent-tool --test secret_e2e`

**M6 — 远程 Providers（Anthropic + OpenAI）+ 回退**（3 天）
- 构建：provider-anthropic（Messages API、SSE、`tool_use`/`tool_result` block、max_tokens 必填）；provider-openai（chat completions SSE、`tool_calls` delta 按 index 累积、忽略 reasoning 字段）；API key 走 SecretRef（`env:ANTHROPIC_API_KEY` 等），配置不落明文；scheduler 回退：本地 worker ModelError（timeout/5xx）⇒ 远程 Frontier 重试一次（`ModelSelected{reason:"fallback"}`）
- 验收：**wiremock 离线测试**：SSE 流解析为 ModelEvent 序列（含 OpenAI 多段 tool-call delta 累积、Anthropic block 式）；auth 失败与 429 路径；回退测试（MockProvider 先失败一次、registry 第二项成功）
- 验证：`cargo test -p provider-anthropic -p provider-openai`（离线）；有 key 才做 live smoke：`agent ask "..." --model claude-sonnet-4-5`

**M7 — MCP**（2 天）
- 构建：agent-mcp 基于 rmcp 3.4.0 pin 死：`~/.agent/config.toml` `[mcp.servers.<name>] { command, args }`（workspace `agent.toml` 覆盖）；启动时 spawn stdio server，`list_tools` → `ToolDefinition{name: "mcp.<server>.<tool>", source: Mcp, risk: Execute}`；`McpTool::invoke` 转换 rmcp content → string，60s 超时，redaction 与其他工具一致
- 验收：测试 fixture = 自写 Rust stdio MCP server（`agent-mcp/tests/` 内，无 npx/网络）；bridge 测试：工具列出、调用、结果往返；kernel e2e：mock provider 请求 `mcp.<server>.<tool>` 得到观察；server 崩溃 ⇒ 工具返回 error 结果，kernel 存活
- 验证：`cargo test -p agent-mcp`（离线，spawn 本地 fixture server）

**M8 — Storage、resume、收尾**（2 天）
- 构建：agent-storage（rusqlite bundled、`~/.agent/agent.db`：`sessions`、`events(session_id, seq, json)`、`tool_audit`；WAL、spawn_blocking）；每事件落库；`agent resume <session-id>` 从事件重建 Session；Ctrl-C 第一次 = 取消当前 step（Cancelled 状态、干净退出）、第二次 = 强制；`agent sessions` 列表；README（中文）含配置参考
- 验收：resume 测试——模拟 2 step 后中断，resume 后模型看到前序上下文（mock 断言请求消息）；事件日志回放重建相同 Session 状态；全 workspace 测试绿
- 验证：`cargo test --workspace && cargo clippy --workspace -- -D warnings`

**M9 —（stretch）llama.cpp**（1–2 天，仅当 M0–M8 全绿）
- 构建：provider-llamacpp 藏 workspace feature `llama-cpp`（默认关）后，eugenehp/llama-cpp-rs **预编译** `x86_64-pc-windows-msvc` artifact（不源码构建），GGUF 路径配置，注册为 local provider
- 验收：feature build 编译过；加载小 GGUF 完成一次补全。若预编译 artifact 在本机有问题，**砍掉 M9**——Ollama 已覆盖本地层，不阻塞 MVP
- 验证：`cargo test -p provider-llamacpp --features llama-cpp`

---

## 5. 关键设计决策

**D1. 事件总线 = 进程内 `tokio::sync::broadcast` + append-only 日志。不用 actor 框架、不用外部 broker。**
`EventBus { tx: broadcast::Sender<AgentEvent>, log: Vec<AgentEvent> }`。Kernel 循环是顺序 async 函数（`Kernel::run_turn`），每次状态转换发事件；(a) 每个状态变化都是事件，(b) 日志是 resume/replay/audit 的唯一事实源（M8 落 SQLite），(c) CLI/telemetry 订阅 broadcast。理由：单用户 CLI 用 actor-per-session 没有任何收益，且让 cancellation/retry 极难测试；顺序循环 + 事件日志用 ~100 行就拿到 §29 的全部收益（resume/replay/audit/debug）。

**D2. Agent loop ↔ scheduler：Kernel 拥有循环，scheduler 是纯决策函数，每 step 调一次（Step 级调度 §10）。**
Kernel 在每 step 前构造 `StepContext`（`needs_tools` 来自上一步工具使用或 control 模型预分类、`complexity`、`escalated`、`turn_classification`）。`scheduler.decide()` 同步纯函数（registry 查询 + policy）——无需 runtime 即可单测。Quality gate 在本地模型 `EndTurn` 之后跑：默认 HeuristicGate；判 Escalate 时 Kernel 以 `escalated=true` 重入 SelectingModel，**每 step 升级预算恰为 1**（循环防护）。理由：把 §11 的两阶段路由实现为贯穿既有状态机的数据流，而不是第二套控制结构。

**D3. CLI 审批 UX：按风险分级，三键 + flags。**
- `Read`（fs read/list）→ 自动放行，不提示。
- `Write`/`Execute`/`Network` → 提示：工具名、格式化 JSON 参数、声明的 secret 引用（仅名字），然后 `approve? [y]es / [n]o / [a]lways-this-session / [d]eny-all-rest`。`a` 把工具名存入 CLI 的 ApprovalPolicy 会话白名单。
- flags：`--approve-all`、`--deny-all`；**非 TTY（管道/CI）默认拒绝高风险工具（fail-closed）**。Deny 产出 `ToolResult{ok:false, error:"denied by user"}`——循环继续，模型可以适应，符合 §17（模型提议、策略决定）。

**D4. SecretRef 延迟绑定与 shell 工具（§18.2 链的具体化）。**
Shell 工具输入 schema：`{ command: string, env?: { [NAME]: "secret:<ref>" } }`——模型只能写**引用**（名字在工具文档里；值永远不可得）。在 ToolRunning 阶段，`ShellTool::invoke` 调 `ctx.broker.env_for(refs)`，解析出的键值对**只注入子进程 env**（`tokio::process::Command::envs`）。值不跨越任何其他边界。纵深防御：(1) 解析值注册进会话 redaction 集合；(2) 每个 `ToolResult.output` 在 `ToolCompleted` 事件存在之前过 `broker.redact()`；(3) M5 测试断言序列化事件零明文。Secret 进入 ModelRequest 在构造上不可能：prompt 组装（D5）从不接触 broker。

**D5. 上下文组装顺序（Prompt Compiler §6）。** System prompt 固定顺序：① AgentProfile（persona、工作风格、编码偏好）→ ② 模型能力说明（tier、"你有没有工具"、该 provider 族输出格式约定）→ ③ WorkspaceProfile（项目类型、语言、构建/测试命令）→ ④ canonical 仓库指令（AGENTS.md/CLAUDE.md/README 摘录）→ ⑤ 匹配的 Skill 指令 → ⑥ 工具使用指引（仅当选中的模型不支持原生工具时——工具以文本协议列出）。然后对话：ContextItem 按 `(priority asc, timestamp asc)` 排序，按预算裁剪（system + 最近 3 条用户消息 + 最近工具观察永不丢）。工具经 provider `tools` 参数传递（当 supports_tools），否则走 ⑥。理由：确定性顺序让 prompt diff/调试可控，符合 §7.1 覆盖顺序（session > repo > workspace > global）。

**D6. MCP 工具作为一等工具并入 Tool Registry。** `agent-mcp` 是**桥**，不是平行执行路径：启动时列出 MCP server 工具，把 `McpTool { client, server, name }` 以 `mcp.<server>.<tool>` 注册进 Kernel 用的同一个 `ToolRegistry`。Kernel、scheduler、审批、redaction、audit 对它们与内置工具一视同仁（risk 默认 Execute ⇒ 走审批；输出走同一 redaction）。agent-mcp 之下没有任何 MCP 专属代码。理由：§14 "能力属于 runtime 不属于 provider"——且这让 M7 成为叶子里程碑，砍掉也不碰 Kernel。

---

## 6. 风险

| # | 风险 | 概率/影响 | 缓解 |
|---|---|---|---|
| R1 | **Windows MSVC 下 embedded llama.cpp**：源码构建已知问题——32 位 `ftell/fseek` 在 MSVC 加载 >4GB GGUF 溢出（仅新版修复）、CRT/链接符号冲突 | 中/高（若强推） | 藏 `llama-cpp` feature 后、**默认关**；用 eugenehp/llama-cpp-rs **预编译** MSVC artifact（不源码构建）；M9 仅 stretch——Ollama 已满足本地层，MVP 完成从不依赖它。预编译不行就砍 M9 |
| R2 | **MCP Rust SDK 成熟度**：rmcp 官方且活跃（3.4.0，已核实 crates.io 最新稳定版），但 1.x→2.x→3.x 有过破坏性变更；协议本身在演进 | 中/中 | workspace deps 里 pin 精确版本；rmcp 类型全部隔离在 agent-mcp（不越桥）；测试用本地 Rust stdio fixture server，不依赖 npx；CLI 里 MCP feature-gate，MCP 构建坏了不阻塞 chat |
| R3 | **Provider 协议差异**：Anthropic `tool_use`/`tool_result` content block + max_tokens 必填 + 消息里无 system role；OpenAI `tool_calls` 以 delta 流式到达需按 index 累积；Ollama 工具调用整块在最终 chunk 送达、结构化输出用 `format:"json"`、qwen3.8 默认发 thinking 需禁用 | 高/中（静默错工具调用） | 每个 provider 在自己的 crate 内归一化为 ModelEvent；**每个 provider 配 wiremock 离线测试**断言精确 ModelEvent 序列；qwen worker `thinking: Some(false)`；JSON-Schema input_schema 作三种格式公分母。Provider 的 bug 结构性困在 provider crate 内 |
| R4 | **Windows 进程/路径**：`cmd.exe /c` 引号（嵌套引号、`&`、`^`）、CWD 与进程继承、junction/8.3 短名绕过 `..` 路径监狱、fs 工具换行符、控制台代码页（GBK vs UTF-8）破坏中文输出 | 中/中 | Shell 工具显式设 cwd、整条命令作单个参数传给 `cmd.exe /c`（不自解析）；监狱检查 = `canonicalize()` + `starts_with(workspace_root)` **且**规范化前拒绝含 `..` 的参数；fs 工具写 `\n` 读两种；子进程 stdout 按字节读、UTF-8 带替换解码（与 chcp 无关）；审批提示与渲染用纯文本（无 ANSI 除非 `--color`） |
| R5 | **Control 模型无工具能力**（translategemma:4b 无 Ollama `tools` 支持） | 确定/低（若遵守） | `ModelCapabilities.supports_tools` 来自 Ollama `/api/show`/tags；scheduler 静态过滤把无工具模型排除出 `needs_tools` step；kernel 请求前断言。M2 在本机实测确认 worker 有 tools、control 无 |
| R6 | **27B Q3 worker 延迟**——多 step turn 很慢；天真超时会杀死合法 step | 中/低 | 模型超时宽（300s）vs 工具超时（60s）；CLI 流式渲染 delta 让用户始终看到进展；`max_steps_per_turn=20` 封顶最坏情况 |

---

## 7. 端到端验证

### M1 验收测试（离线）

文件：`crates/agent-kernel/tests/e2e_mock.rs` ——**无网络、无 Ollama、无 API key**：
`cargo test -p agent-kernel --test e2e_mock`

场景：用户问 "What does the echo tool say with 'hello'?"，mock 模型先发工具调用，看到观察后再发最终回答。

```rust
#[tokio::test]
async fn user_message_to_tool_call_to_answer() {
    let tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool));          // 内置测试工具：返回 args.text

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

    // 1. 完整生命周期，按序：
    assert_eq!(kinds, vec![
        SessionStarted, UserMessageReceived, TurnStarted,
        StepStarted, ContextPrepared, ModelSelected, ModelStarted,
        ModelDelta, ModelCompleted,          // step 1: 工具调用
        ToolRequested, ToolApproved, ToolStarted, ToolCompleted,
        ObservationCreated, StepCompleted,
        StepStarted, ContextPrepared, ModelSelected, ModelStarted,
        ModelDelta, ModelCompleted,          // step 2: 最终回答
        StepCompleted, TurnCompleted,
    ]);

    // 2. 观察确实来自工具执行：
    let obs = events.iter().find_map(|e| e.tool_result()).unwrap();
    assert!(obs.ok && obs.output == "hello");

    // 3. Step 2 的请求确实包含该观察（循环回喂）：
    let req2 = kernel.last_requests().last().unwrap();
    assert!(req2.messages.iter().any(|m| m.role == Role::Tool && m.content == "hello"));

    // 4. 最终回答浮出：
    let answer = events.iter().find_map(|e| e.turn_answer()).unwrap();
    assert!(answer.contains("hello"));
}
```

这一个测试在离线状态下证明 §37 的核心主张：**用户消息 → 模型 → 工具调用 → 观察 → 模型 → 最终回答**，事件日志完整到足以支撑 M8 的 resume/replay 与 M5 的 secret 扫描（复用同一事件向量）。

### 全局收尾验证（M8 后）

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo run -p agent-cli -- ask "1+1等于几" --provider ollama --model translategemma:4b
```

---

## 关键文件

- `crates/agent-kernel/src/kernel.rs` —— agent loop + `run_turn`，其余一切挂于其上（旁有 `state.rs`）
- `crates/agent-model/src/provider.rs` —— `ModelProvider`/`ModelRequest`/`ModelStream`/`ModelEvent`，所有 provider 与 mock 实现的供应商中立接缝
- `crates/agent-core/src/event.rs` —— `AgentEvent`，resume、audit、redaction 扫描与 e2e 测试断言的契约
- `crates/agent-tool/src/shell.rs` —— shell 工具（路径监狱、超时、延迟绑定 secret env 注入），最高风险内置工具
- `crates/agent-kernel/tests/e2e_mock.rs` —— 离线端到端验收测试，M1 与整个 MVP 的门禁
