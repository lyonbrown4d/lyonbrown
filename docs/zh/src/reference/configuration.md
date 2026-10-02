# 配置参考

Lyonbrown 使用 `config-rs` 分层加载 TOML、YAML、JSON、JSON5、RON 和 INI。优先级从低到高为：内置默认值、`~/.agent/config.toml`、工作区 `agent.toml`、`--config` 指定文件、`LYONBROWN_` 环境变量、CLI 参数。嵌套环境变量用双下划线，例如 `LYONBROWN_MODEL__PROVIDER=ollama`。工作区配置中的 profile、模型路由、Provider endpoint、远程凭据、审批与存储设置默认拒绝；确认信任后加 `--trust-workspace-config`。

## Ollama

```toml
[model]
provider = "ollama"
model = "qwen3.8-27b:q3-claude"
max_tokens = 2048

[ollama]
endpoint = "http://127.0.0.1:11434"
```

直接运行 `agent` 即可打开工作区 chat TUI，`agent .` 是对应的显式写法。
不再保留 `ask`、`chat`、`sessions` 或 `resume` 兼容子命令。TUI 与 GUI 通过
同一套已认证 gRPC 契约连接 `agentd`；只有 `agentd` 创建 Kernel 并持有 Runtime
状态。

## 远程模型与自动回退

API key 只配置引用，不要写明文。`env:` 从进程环境读取，`file:` 从 `~/.agent/secrets/<name>` 读取；Provider 在发送请求时才解析引用。

```toml
[model]
provider = "ollama"

[anthropic]
enabled = true
endpoint = "https://api.anthropic.com"
api_key_ref = "env:ANTHROPIC_API_KEY"

[openai]
enabled = true
endpoint = "https://api.openai.com/v1"
api_key_ref = "env:OPENAI_API_KEY"
```

默认未启用远程 Provider。设 `model.provider = "anthropic"` 或 `"openai"` 可直接选用对应 Provider；也可保持本地 Provider，启用远程 Provider 后，在本地 Worker 建立流连接或首个输出前遇到超时或 HTTP 5xx，Kernel 会按路由策略向远程 Frontier 重试一次。自动回退仅适用于 Public 上下文；工具结果及标记为 Confidential/Secret 的上下文不会自动发往远程。显式选择远程 Provider 仍由用户自行决定数据边界。401/429、已有模型输出后的错误不触发回退。

仓库中的 `agent.toml` 可以指定 profile、Provider 和服务 endpoint。CLI 默认拒绝 profile、模型、审批策略与存储设置，需显式传入 `--trust-workspace-config` 后才会加载；仅当你信任该仓库配置时启用。

## 工具审批

```toml
[approval]
approve_all = false
deny_all = false
timeout_seconds = 300
```

交互审批默认在 300 秒后拒绝，可通过 `approval.timeout_seconds` 调整；值必须大于零。客户端断开、响应 ID 不匹配或超时都按拒绝处理。TUI 的审批默认动作是 Deny once，`Enter` 直接执行该安全动作。

## MCP 工具

```toml
[mcp.servers.files]
command = "C:/path/to/mcp-server.exe"
args = ["--stdio"]
```

`agentd` 连接已配置的 stdio server 并注册 `mcp.files.<tool>` 工具。MCP 工具
默认风险为 Execute，仍走统一审批、60 秒超时和输出脱敏。工作区 `agent.toml`
中的 MCP 命令默认拒绝启动；审阅并信任该工作区后，运行
`agent --trust-workspace-mcp`。模型路由与 MCP 命令使用独立信任标志。全局或
显式 `--config` 中的 MCP 配置视为用户已信任。推荐使用明确的可执行文件路径。

## 会话存储与恢复

默认开启 SQLite WAL 存储，路径为 `~/.agent/agent.db`，并由 `agentd` 持有。
Session 列表和恢复是基于 gRPC 的 client 交互，不再通过直读存储或旧 CLI 子命令
完成。可用以下配置关闭或改路径：

```toml
[storage]
enabled = true
path = "./.agent/session.db"
```

相对存储路径以工作区为基准；也可设置 `LYONBROWN_STORAGE__ENABLED=false` 或 `LYONBROWN_STORAGE__PATH`。事件写入在提交到数据库后才返回成功；中断时恢复会保留已完成的工具结果，并把执行中但未记录结果的调用标为结果未知，不会自动重放。TUI 运行时第一次 Ctrl-C 取消当前轮，第二次强制停止；Esc 不退出，使用 Ctrl-D 或 `/quit` 明确退出。

## 由 daemon 持有的 llama.cpp

llama.cpp 是默认 Provider，也是 `agentd` 的默认 feature。daemon 直接在自身
进程内加载 GGUF，不需要独立的 HTTP 推理服务，也不会启动外部推理进程。
`cuda` 或 `vulkan` feature 可选；使用 `--no-default-features` 的精简 `agentd`
构建不会包含它。client 不加载 GGUF，也不回退到内嵌 Kernel。

```toml
[model]
provider = "llamacpp"
model = "qwen-local"
max_tokens = 1024
thinking = false

[llamacpp]
model_path = "models/qwen.gguf"
context_size = 8192
max_output_tokens = 2048
batch_size = 512
threads = 8
batch_threads = 8
gpu_layers = 0
temperature = 0.7
seed = 0
```

相对模型路径以工作区为基准。也可以设置
`LYONBROWN_LLAMACPP__MODEL_PATH`。`model.thinking` 默认是 `false`；对于
Qwen3 模板，provider 会在渲染前应用官方 `/no_think` 控制。省略
`model_path` 时，`agentd` 会检查默认 `qwen3-1.7b-q4_k_m` 的已校验缓存；
若缓存缺失，Runtime 进入 `ModelSetupRequired`，打开 client 不会触发下载。
daemon 已提供显式后台 Start/Cancel RPC，并在统一的 `RuntimeSnapshot` stream
发布进度；TUI/GUI controls 属于下一小节点。配置的 `model_path` 缺失时不提供
托管下载。详见
[本地模型与任务路由](local-models-routing.md)。Windows 原生构建需要 MSVC、
Windows SDK、CMake/Ninja 和可被绑定生成器发现的 LLVM `libclang`；必要时设置
`LIBCLANG_PATH`。

```powershell
.\scripts\build-windows.ps1
.\target\debug\agent.exe --config agent.toml .
```

该脚本会原地修复中断的 llama.cpp CMake generation，默认不清理或另建
`target`，并同时生成同目录的 `agent.exe` 与 `agentd.exe`；只执行
`cargo run -p agent-cli` 不是完整的 C/S 开发启动方式。

在未包含该 feature 的 daemon 中选择 `llamacpp` 时会明确报错，不会回退到
外部服务。真实模型验收可设置 `LYONBROWN_GGUF_MODEL` 后运行 provider 中标记
为 ignored 的 live smoke test。
