# Lyonbrown Local-First Agent Runtime 架构设计

> 本文定义长期 Kernel 与领域架构。产品部署和进程所有权以
> [本地 gRPC C/S 架构](grpc-client-server.md)为准：`agentd` 是 Runtime/模型的
> 唯一所有者，TUI 与 GUI 都是经过认证的 gRPC client。旧图中作为适配器出现的
> CLI/Desktop/Web 不表示允许内嵌 Runtime 兼容路径。

> 版本：Draft v0.1  
> 状态：长期架构方向稿；当前实施进度见 [第一阶段 MVP 实施计划](../plans/mvp-implementation-plan.md)
> 定位：长期维护的、模型供应商中立（Model Provider Neutral）、本地优先（Local-first）的通用 Agent Runtime  
> 建议主实现语言：Rust

---

## 1. 项目定位

本项目目标不是再实现一个绑定特定模型或特定场景的 Coding Agent，而是构建一个长期可维护的 **通用 Agent Runtime / Agent Kernel**。

核心思想：

> **Agent 拥有身份、上下文、能力、工具、密钥和工作状态；模型只是可替换的计算资源。**

模型不应该拥有 Agent，Agent Runtime 应该拥有模型。

因此整个系统需要天然支持：

- 本地模型
- 远程模型
- 多模型协同
- 会话内模型切换
- 不同模型之间无缝接管
- 本地优先执行
- 本地模型失败后远程升级
- 本地小模型作为控制模型
- 本地大模型作为主要 Worker
- 远程 Frontier Model 作为稀缺高价值算力
- 工具与能力独立于模型供应商
- Agent 身份和工作风格独立于具体会话
- Secret 不进入模型上下文
- 本地优先运行，可选官方 Sync Server，也允许完全自建

---

## 2. 非目标

第一阶段不追求：

- 从零实现模型推理引擎
- 自己实现 llama.cpp / vLLM / Ollama
- 自己实现向量数据库
- 自己实现通用网络协议
- 自己实现所有模型 Provider SDK
- 复制现有 Agent 成品或半成品 Runtime

允许并鼓励复用基础库，例如：

- Tokio
- Axum
- Reqwest
- Serde
- tracing / OpenTelemetry
- Wasmtime
- MCP SDK
- Rig Core 或其他 Provider 抽象库
- SQLite / PostgreSQL / RocksDB 等存储库

原则：

> **架构自己设计，基础能力尽量复用成熟库。**

---

## 3. 总体架构

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
│                                                             │
│ Shell                                                       │
│ Filesystem                                                  │
│ Browser                                                     │
│ Computer Use                                                │
│ Code Execution                                              │
│ Git                                                         │
│ MCP                                                         │
│ HTTP                                                        │
│ Database                                                    │
│ Custom Capabilities                                         │
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

## 4. 核心原则

### 4.1 Model Provider Neutral

Agent Runtime 不应绑定：

- OpenAI
- Anthropic
- Gemini
- DeepSeek
- Qwen
- Ollama
- llama.cpp
- vLLM

统一抽象：

```rust
trait ModelProvider {
    async fn stream(
        &self,
        request: ModelRequest,
    ) -> Result<ModelStream>;
}
```

模型身份至少拆成：

```text
ProviderId
ModelId
ModelCapabilities
```

禁止把 Provider 写死成固定 enum。

---

### 4.2 Agent Session 属于 Runtime，而不是 Provider

错误模式：

```text
Session ≈ Claude Conversation
Session ≈ OpenAI Conversation
```

正确模式：

```text
Your Session
 ├── Step 1 -> Local Small Model
 ├── Step 2 -> Local Large Model
 ├── Step 3 -> Claude
 ├── Step 4 -> GPT
 └── Step 5 -> Local Large Model
```

模型切换不能导致：

- 工作风格丢失
- Workspace 状态丢失
- Skill 丢失
- 当前 Plan 丢失
- Tool 状态丢失
- 用户偏好丢失

---

## 5. Agent Identity Runtime

Agent 的身份与工作方式需要脱离具体 Session。

建议持久化：

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

示例：

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

Agent Profile 不应简单拼进 System Prompt。

需要单独的：

```text
Agent Profile
      +
Workspace Policy
      +
Skill Instructions
      +
Model Capabilities
      ↓
Prompt Compiler
      ↓
Provider-specific Prompt
```

原因：

不同模型：

- 对 System Prompt 结构敏感度不同
- Tool Calling 协议不同
- Reasoning 参数不同
- Structured Output 能力不同
- Prompt 最佳实践不同

Prompt Compiler 应负责把统一 Agent Identity 转成不同 Provider 最适合的 Prompt。

---

## 7. Workspace Runtime

新仓库不应强制要求手工创建 `AGENTS.md`。

进入仓库后自动识别：

```text
Cargo.toml
go.mod
pom.xml
build.gradle
package.json
docker-compose.yml
README.md
Git History
Directory Structure
```

生成：

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

默认存储：

```text
~/.agent/workspaces/<workspace-id>/
```

不污染 Git 仓库。

---

### 7.1 兼容现有 Agent 规则文件

自动识别：

```text
AGENTS.md
CLAUDE.md
CODEX.md
.github/copilot-instructions.md
.cursor/*
README.md
CONTRIBUTING.md
```

统一解析成：

```text
Canonical Repository Instructions
```

配置覆盖顺序建议：

```text
Session Override
      >
Repository
      >
Workspace
      >
Organization
      >
Global Profile
```

---

## 8. Skill Runtime

Skill 不应该依赖用户手工“调用”。

设计为：

```text
Skill Registry
      ↓
Intent / Capability Matcher
      ↓
Auto Activation
```

Skill 定义：

```yaml
name: quarkus-debug

triggers:
  - quarkus
  - startup failure
  - extension error

requires:
  tools:
    - shell
    - filesystem

context:
  files:
    - pom.xml
    - build.gradle
    - application.yaml
```

Skill 应是：

```text
Global Asset
```

而不是每个 Session 重新声明。

---

## 9. Model Runtime

整个模型系统建议分成三层。

### 9.1 Local Control Model

建议：

```text
2B ~ 4B
llama.cpp
CPU / iGPU
短输出
Structured Output
```

职责：

- Intent Classification
- Complexity Estimation
- Tool Selection
- Context Selection
- Context Compression
- Query Rewrite
- Skill Selection
- Result Classification
- Escalation Suggestion

它不是主要负责最终回答的模型。

---

### 9.2 Local Worker Model

例如：

```text
Qwen 20B ~ 30B
llama.cpp / Ollama / vLLM
GPU
```

职责：

- 普通推理
- Coding
- Debug
- Plan
- Tool Use
- 文档
- 本地任务执行

目标：

> 尽量让绝大多数普通任务在本地完成。

---

### 9.3 Remote Frontier Model

例如：

```text
Claude
GPT
Gemini
其他高能力远程模型
```

职责：

- 高复杂度推理
- 本地模型失败后的升级
- 大型架构分析
- 高价值复杂任务
- 本地能力不足的多模态任务

定位：

> 稀缺、高价值算力。

---

## 10. Model Scheduler

不采用：

```text
一个 Session 固定一个模型
```

而采用：

```text
Step-level Model Scheduling
```

模型是 Agent Kernel 可调度资源。

```text
Agent Step
    │
    ▼
Static Policy
    │
    ├── Privacy
    ├── Capability
    ├── Budget
    └── Locality
    │
    ▼
Complexity Router
    │
    ▼
Model Selection
```

模型选择条件：

```text
Task Capability
Privacy Level
Data Classification
Context Size
Latency
Cost
Local Availability
Model Capability
Current Load
Historical Success Rate
```

---

## 11. 两阶段 Routing

只做 Pre-Routing 不够。

需要：

```text
          Agent Step
              │
              ▼
      Pre-Execution Router
              │
              ▼
        Selected Model
              │
              ▼
           Result
              │
              ▼
         Quality Gate
          /        \
       PASS      ESCALATE
        │            │
        ▼            ▼
     Continue     Stronger Model
```

本地模型先执行。

执行后判断：

- 是否完成目标
- 是否违反格式
- Tool Result 是否合理
- 是否出现不确定性
- 是否需要强模型校验

再决定是否升级。

---

## 12. Context Engine

Context 不能简单定义为：

```text
Vec<Message>
```

建议定义统一：

```text
ContextItem
```

类型：

```text
Message
ToolObservation
Document
Memory
Summary
Artifact
Reference
WorkspaceState
Plan
Finding
```

每个 Item 包含：

```text
priority
token_estimate
source
timestamp
confidence
expandable
classification
```

---

### 12.1 Progressive Context Disclosure

禁止直接：

```text
100K Raw Context
      ↓
Remote Model
```

推荐：

```text
Raw Context
   ↓
Deterministic Filter
   ↓
Semantic Retrieval
   ↓
Local Compression
   ↓
Context Budget
   ↓
Model
```

例如：

```text
100K
 ↓
35K
 ↓
15K
 ↓
5K
```

压缩结果必须保留 Source Reference。

```json
{
  "summary": "数据库连接池在重试风暴后耗尽",
  "sources": ["tool://logs/request-92#L820-L1160", "file://config/db.yaml#L31-L58"]
}
```

模型需要时可以：

```text
expand(source)
```

重新取得原始内容。

---

## 13. Memory 分层

必须明确区分：

```text
Conversation
!=
Working Memory
!=
Long-term Memory
```

建议：

```text
Session Event Log
├── Messages
├── Tool Calls
├── Decisions
├── Artifacts
└── Observations

Working Memory
├── Current Goal
├── Current Plan
├── Findings
├── Pending Tasks
└── Temporary State

Long-term Memory
├── User Preferences
├── Workspace Knowledge
├── Reusable Knowledge
└── Historical Decisions
```

---

## 14. Capability Runtime

能力不应该属于模型供应商。

统一：

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

因此：

```text
Computer Use
```

不是 Codex 独有能力。

它应该是 Runtime Capability。

---

## 15. Computer Use

统一抽象：

```rust
trait ComputerProvider {
    async fn observe(
        &self,
        session: &ComputerSession,
    ) -> Result<ComputerObservation>;

    async fn execute(
        &self,
        action: ComputerAction,
    ) -> Result<ComputerObservation>;
}
```

动作：

```rust
enum ComputerAction {
    Click { x: u32, y: u32 },
    DoubleClick { x: u32, y: u32 },
    Type { text: String },
    Key { key: String },
    Scroll { dx: i32, dy: i32 },
    Drag { from: Point, to: Point },
}
```

Backend 可以是：

```text
Local Desktop Driver
Browser / Playwright
Remote Computer Driver
Vision-based Driver
Third-party CUA Provider
```

Agent 只依赖 Capability，不依赖具体模型。

---

## 16. 多模型 Computer Use

不要求一个模型完成所有工作。

可以：

```text
Screenshot
    ↓
Local Vision Model
    ↓
Screen Semantic State
    ↓
Reasoning Model
    ↓
Computer Action
```

Agent 可以组合：

```text
Control Model
Reasoning Model
Vision Model
Embedding Model
Judge Model
```

---

## 17. Tool Runtime

统一抽象：

```text
ToolDefinition
ToolInvocation
ToolResult
```

职责：

```text
Registry
Permission
Approval
Execution
Timeout
Retry
Redaction
Audit
```

模型只能提出：

```text
Tool Call Intent
```

真正是否允许执行，由 Policy Engine 决定。

---

## 18. Secret Broker

Secret 是一级核心能力。

原则：

> Secret 永远不属于模型上下文。

无论：

```text
Claude
GPT
Qwen
Local llama.cpp
```

默认都不能直接看到 Secret。

---

### 18.1 SecretRef

模型只能使用逻辑引用：

```text
payment.prod.credentials
github.company
postgres.prod.readonly
```

或者：

```text
secret://payment/prod/private-key
```

不返回真实值。

---

### 18.2 Late Secret Binding

正确执行链：

```text
Model Planning
      ↓
Tool Call
      ↓
Policy Check
      ↓
Secret Resolution
      ↓
Execution
```

Secret 只在最后一个可信执行阶段绑定。

---

## 19. 支付集成测试示例

模型发起：

```json
{
  "tool": "payment.integration_test",
  "arguments": {
    "environment": "production",
    "merchant": "main",
    "amount": 0.01
  }
}
```

Tool 定义：

```yaml
payment.integration_test:
  requires:
    secrets:
      - payment.main.api_key
      - payment.main.private_key
```

执行：

```text
Agent
 ↓
Tool Request
 ↓
Policy Engine
 ↓
Human Approval
 ↓
Secret Broker
 ↓
Local Signer
 ↓
HTTP Request
 ↓
Response Redaction
 ↓
Model
```

模型永远不知道：

```text
private_key
api_key
signature secret
```

---

## 20. Secret Scope

Secret 必须有能力边界。

```yaml
secret: payment.prod.private_key

allow:
  tools:
    - payment.query
    - payment.integration_test

deny:
  tools:
    - shell
    - raw_http
    - browser

environment:
  - production
```

即使模型执行：

```text
shell: print PAYMENT_PRIVATE_KEY
```

也无法读取。

---

## 21. Secret Provider

支持：

```text
OS Keychain
Local Encrypted Store
HashiCorp Vault
1Password
Bitwarden
AWS Secrets Manager
Kubernetes Secrets
Custom Provider
```

不建议把所有 Secret 直接放入 Agent 进程环境变量。

---

## 22. Secret 不一定允许读取

更高安全级别：

禁止：

```rust
fn get_secret() -> String
```

更推荐：

```text
sign(secret_ref, payload)
execute_with_secret(tool, secret_ref)
```

例如私钥：

```text
Agent
 ↓
Request Signature
 ↓
Secret Provider / HSM / Vault Transit
 ↓
Signature
```

Agent 进程也不获得私钥原文。

---

## 23. Output Redaction

不仅输入不能泄密。

工具返回值也必须：

```text
Raw Tool Result
      ↓
Secret Scanner
      ↓
Redaction
      ↓
Model Observation
```

例如：

```json
{
  "access_token": "[REDACTED]",
  "order_id": "202609210001"
}
```

---

## 24. Data Classification

建议统一：

```text
PUBLIC
INTERNAL
CONFIDENTIAL
SECRET
```

Model Scheduler 根据级别执行：

```text
PUBLIC
   -> Remote allowed

INTERNAL
   -> According to organization policy

CONFIDENTIAL
   -> Local preferred / trusted remote only

SECRET
   -> Never enter model context
```

---

## 25. Sync Server

Sync Server 是：

```text
Optional
```

不是 Agent Runtime 硬依赖。

Local-first 是默认工作模式。

支持：

```text
Offline Only
Official Hosted Sync
Self-hosted Sync
```

---

## 26. Sync Provider 抽象

```rust
trait SyncProvider {
    async fn push(
        &self,
        objects: Vec<SyncObject>,
    ) -> Result<()>;

    async fn pull(
        &self,
        cursor: SyncCursor,
    ) -> Result<SyncBatch>;
}
```

实现：

```text
NoopSync
LocalFolderSync
OfficialCloudSync
SelfHostedSync
```

Agent 在没有任何账号、没有服务器的情况下必须完整可用。

---

## 27. Sync 数据分级

### Level 1：普通配置

可以同步：

```text
Agent Profile
Working Style
UI Preferences
Model Preferences
Skill Settings
Policy
```

### Level 2：Workspace Metadata

可以同步：

```text
Repo Fingerprint
Architecture Summary
Build Commands
Enabled Skills
Project Conventions
```

可配置：

```text
workspace.private = true
```

### Level 3：敏感记忆

建议：

```text
Client-side Encryption
```

### Level 4：Secret

默认：

```text
Local Only
```

如果允许同步：

```text
Explicit Opt-in
+
End-to-End Encryption
```

更推荐对接独立 Secret Manager，而不是让 Sync Server 变成密码管理器。

---

## 28. Sync Server 目标

官方 Sync Server 主要同步：

```text
Identity
Profile
Preferences
Skills
Workspace Metadata
Memory Metadata
Session Index
Device State
```

而不是成为 Agent 运行的中心。

---

## 29. Event-driven Agent Kernel

Kernel 内部建议采用：

```text
Event Driven
+
State Machine
```

而不是简单无限循环。

事件：

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

优势：

```text
Resume
Replay
Audit
Debug
Fork
Evaluation
Checkpoint
```

---

## 30. Agent State Machine

```text
IDLE
 ↓
PREPARING_CONTEXT
 ↓
SELECTING_MODEL
 ↓
MODEL_RUNNING
 ↓
┌───────────────────────┐
│                       │
↓                       ↓
TOOL_REQUIRED         COMPLETED
│
↓
TOOL_RUNNING
│
↓
OBSERVATION
│
└────────→ SELECTING_MODEL
```

---

## 31. Rust 模块划分建议

```text
agent-runtime/
│
├── crates/
│
│   ├── agent-core/
│   │   ├── identity
│   │   ├── session
│   │   ├── turn
│   │   ├── step
│   │   └── event
│
│   ├── agent-kernel/
│   │   ├── runtime
│   │   ├── state-machine
│   │   ├── cancellation
│   │   ├── retry
│   │   ├── resume
│   │   └── sub-agent
│
│   ├── agent-model/
│   │   ├── provider
│   │   ├── registry
│   │   ├── capabilities
│   │   ├── scheduler
│   │   └── policy
│
│   ├── agent-context/
│   │   ├── memory
│   │   ├── retrieval
│   │   ├── compression
│   │   └── budget
│
│   ├── agent-profile/
│   │
│   ├── agent-workspace/
│   │
│   ├── agent-skill/
│   │
│   ├── agent-capability/
│   │
│   ├── agent-tool/
│   │
│   ├── agent-secret/
│   │
│   ├── agent-sync/
│   │
│   ├── agent-mcp/
│   │
│   ├── agent-storage/
│   │
│   ├── agent-sandbox/
│   │
│   └── agent-telemetry/
│
├── providers/
│   ├── openai/
│   ├── anthropic/
│   ├── ollama/
│   └── llama-cpp/
│
├── apps/
│   ├── agent-cli/
│   ├── agent-server/
│   └── agent-desktop/
│
└── examples/
```

---

## 32. Rust 技术栈建议

核心：

```text
Rust
Tokio
Serde
thiserror / anyhow
tracing
OpenTelemetry
```

服务端：

```text
Axum
Tower
Reqwest
```

插件 / Sandbox：

```text
Wasmtime
WASI
```

存储：

```text
SQLite
PostgreSQL
RocksDB / Redb（按需求）
```

模型：

```text
OpenAI-compatible HTTP
Anthropic API
llama.cpp
Ollama
vLLM
```

可以考虑复用：

```text
Rig Core
```

作为 Provider 抽象或参考，但不直接把整个 Agent Runtime 建立在 Rig Agent 上。

---

## 33. 插件模型

不建议依赖：

```text
Rust dylib
```

作为主要第三方插件机制。

建议优先：

```text
WASM Component
MCP
JSON-RPC
gRPC
Process Plugin
```

目标：

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

Agent 最终需要：

```text
Process Sandbox
Docker Sandbox
WASM Sandbox
Remote Worker
Workspace Isolation
```

尤其第三方 Skill / Tool 不应默认拥有宿主机完整权限。

---

## 35. Observability

所有模块统一产生：

```text
Trace
Metric
Event
Cost
Token Usage
Model Route
Tool Call
Latency
Error
Escalation
```

核心指标：

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

## 36. 最重要的产品体验

### 36.1 换模型不断档

```text
Local Qwen
     ↓
Claude
     ↓
GPT
     ↓
Local Qwen
```

用户不需要重新：

```text
解释工作习惯
加载 Skill
描述 Repository
重新提供 Agent Rules
```

---

### 36.2 新仓库 0 配置

```bash
agent .
```

自动：

```text
Load Global Profile
Detect Repository
Load Repository Instructions
Build Workspace Profile
Match Skills
Start Session
```

---

### 36.3 Secret 永不暴露给模型

```text
Agent wants action
      ↓
Tool
      ↓
SecretRef
      ↓
Trusted Execution
```

而不是：

```text
Secret
 ↓
Prompt
```

---

### 36.4 Computer Use 与模型解耦

任何满足能力要求的模型都可以使用：

```text
Computer Capability
```

而不是只有某一个模型供应商才能操作桌面。

---

## 37. 推荐第一阶段 MVP

第一阶段不要一次做完所有能力。

建议只实现：

```text
Agent Kernel
Session/Event
Agent Profile
Workspace Runtime
Skill Registry
Model Provider
Model Scheduler
Local Control Model
Local Worker Model
Remote Fallback
Tool Registry
Shell
Filesystem
MCP
SecretRef
Basic Secret Broker
CLI
```

模型：

```text
Local Control:
Qwen 2B ~ 4B
llama.cpp

Local Worker:
Qwen 20B ~ 30B

Remote:
任意 1~2 个 Frontier Provider
```

---

## 38. 第二阶段

增加：

```text
Context Compression
Long-term Memory
Sync Provider
Official Sync Server
Self-hosted Sync
Computer Use
Browser Use
WASM Plugin
Sandbox
Human Approval
Data Classification
Advanced Secret Broker
```

---

## 39. 第三阶段

增加：

```text
Multi-Agent
Remote Worker
Distributed Execution
Organization Policy
Enterprise Secret Manager
Agent Marketplace
Skill Marketplace
Evaluation System
Automatic Route Calibration
Model Performance History
```

---

## 40. 最终架构哲学

整个系统最核心的原则可以总结为：

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

也就是说：

> **Agent 是长期存在的主体，模型只是随时可以替换的推理后端。**

进一步：

> **本地模型不是廉价版远程模型，而是 Agent Control Plane 的一部分。**

最终形成：

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

## 41. 项目一句话定位

> **A local-first, provider-neutral Agent Runtime where identity, context, capabilities and secrets belong to the Agent — models are replaceable compute providers.**

中文：

> **一个本地优先、模型供应商中立的通用 Agent Runtime：身份、上下文、能力和密钥属于 Agent，模型只是可替换的计算资源。**
