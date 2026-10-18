# 本地工具与浏览器操作

模型工具由 agentd 装配，客户端只显示调用、审批和结果。工具结果、网页内容及进程输出均不能授予额外权限。远程会话中的本地命令在 daemon 所在主机执行。

## 启用受信任命令

`execute_command` 默认关闭。管理员在 daemon 的 `config.toml` 中显式列出可执行文件的绝对路径，重启后才向模型提供此工具。例如以下 macOS 配置允许 `/usr/bin/printf`，并只向子进程传入指定的环境引用：

```toml
[commands]
executables = ["/usr/bin/printf"]

[commands.environment]
CHILD_KEY = "LYONBROWN_CHILD_VALUE"
```

`LYONBROWN_CHILD_VALUE` 必须存在于分层配置读取的 dotenv 或进程环境中。右侧是变量名，不能填写真实凭证。未解析的引用使配置加载失败。子进程不继承完整环境，参数直接传递给程序，不经过 shell；可执行文件和环境映射分别最多 64 项。

工作目录必须位于会话工作区内，默认为 `.`。这项约束不隔离程序的文件和网络访问：受信任程序仍拥有运行 agentd 的操作系统用户权限。允许解释器或 shell 会允许其参数表达更广泛的操作，应由管理员评估允许的程序及每次准确参数。

| 模型参数 | 默认值与限制 |
| --- | --- |
| `executable` | 必填，必须匹配绝对路径允许列表 |
| `arguments` | 空数组，最多 256 个字符串 |
| `cwd` | `.`，相对会话工作区 |
| `timeout_ms` | `30000`，允许 `1..=120000` 毫秒 |
| `output_limit_bytes` | `65536`，允许 `1..=1048576` 字节 |

每次执行都要求人工批准准确动作，审批绑定会话、`owner_epoch`、运行、调用、动作标识、解析后的程序路径、参数、目录、超时和输出上限。daemon 再次核对会话所有权和运行状态，先保存执行意图再启动程序，随后保存结果回执。取消、执行结果或回执持久化无法确认时进入 `Uncertain`；不能因断线或相同调用再次出现而自动重放。核对持久回执与实际副作用后再决定下一步。

依据：`crates/agent-config/src/commands.rs`、`crates/agent-workspace/src/process/`、`apps/agentd/src/commands/`。回归入口为 `apps/agentd/tests/command_tools.rs`。

## 模型浏览器工具

`browser_observe` 列出当前会话的 Tab，或观察指定 Tab 的快照、修订号、可访问性数据和可选截图 artifact 引用。返回内容带有不可信标记。Tab 列表移除 URL 查询和 fragment，不应把页面文字当作系统授权。

`browser_act` 接收 `call` 对象，支持以下操作：

| 操作 | 必需条件 |
| --- | --- |
| `open_space` | 指定 `profile_id`，选择已连接 runtime host；可指定 `host_id`、`headless` |
| `open_tab` | 当前会话的 `space_id` 与允许的 URL |
| `close_space` | 当前会话的 `space_id` |
| `click` / `fill` / `press` | `tab_id`、当前 `snapshot_id`、`revision` 及观察返回的 `element_ref` |
| `navigate` | `tab_id`、当前 `snapshot_id`、`revision`，以及 `text` 中的目标 URL |

变更操作要求准确动作人工审批。daemon 检查会话、所有者代次、runtime host 绑定、取消状态和人工接管状态。人工接管期间暂停模型变更；模型工具不开放原始输入和接管能力。快照过期后重新观察，未知结果先核对记录，禁止自动重放。Profile 保存不代表活页面恢复。

模型侧变更在同一本地事务中消费准确审批、保存浏览器执行意图，并写入事件和 Outbox；事务再次检查运行仍在执行、会话及所有者代次一致。事务失败时审批和意图一同回滚。提交后才向获批的 runtime host 发送操作；等待回执时取消或断线会保留 `Uncertain`，不重放动作。客户端的人工浏览器操作使用独立的认证入口，并保存相同类型的执行意图和结果回执。

原生 CEF 语义动作在滚动、聚焦、按键或导航之前撤销旧快照。进入这些步骤后，动作或后续观察失败会进入 `Uncertain`，停止本次模型运行，并保留持久回执供人工核对。输入、所有权和快照检查在动作之前完成。

人工接管的原始输入在聚焦或派发前撤销旧语义快照；进入这些步骤后的错误保存为 `Uncertain`，恢复不自动重放。成功输入本身不递增帧版本，允许同一帧的按下与松开配对；页面变化仍可使帧失效。

`apps/agentd/tests/browser_tools.rs` 通过本地 HTTP 模型 mock 和真实协议适配器覆盖 Responses、Chat Completions、Anthropic Messages，以及批准、拒绝、所有权/会话边界、取消和截图 artifact。其 runtime host 是合成实现，验证的是 Harness 到 daemon 的工具契约与持久状态，不能替代原生 CEF 自主浏览器操作验收。

原生链路使用 `apps/agentd/tests/native_browser_tools.rs` 单独验收。在 macOS 27.0.1 arm64 的调试 CEF 环境中，三个协议分别完成真实 AX 观察、准确审批后的填入与点击，并通过本地 HTTP 页面副作用和持久回执核对结果；另三种场景在点击成功后丢弃回执，确认取消与恢复不重放，运行保持 `Uncertain`。测试只操作合成页面和独立 Profile，使用调试测试钥匙串，不证明生产钥匙串或其他平台已验收。

`apps/agentd/tests/native_browser_post_effect.rs` 通过 Responses 协议验证点击后的页面错误：一次实际点击产生一次本地 HTTP 副作用，随后超限页面内容触发错误。测试确认运行与浏览器回执持久化为 `Uncertain`、模型停止继续执行、恢复不重放，并拒绝在未核对结果的会话中启动新运行。该场景与上述六个场景均使用重新构建并签名的原生宿主执行。

同一调试环境还通过 `apps/agentd/tests/native_browser_human_input.rs` 验证人工接管：鼠标按下与松开只产生一次页面副作用，错误回执和 Outbox 保存为 `uncertain`，恢复不重放，也不启动模型运行。`crates/agent-browser-runtime/tests/cef_runtime.rs` 验证正常的同帧鼠标输入和文本提交。

```sh
cargo test -p agentd --test browser_tools --test command_tools --locked
```

原生运行截图的覆盖范围见[桌面运行截图与验收范围](runtime-evidence.md)。浏览器工具实现位于 `apps/agentd/src/browser/agent_tools/`，BrowserRuntime 生命周期契约见[协议与平台能力](protocol.md)。

## 委派给配对节点

`delegate_task` 默认关闭。先通过 daemon 的 `peer-pair` 操作核对完整 TLS 与 Noise 配对指纹，再在 `config.toml` 中列出允许的节点、HTTPS 地址与能力。发现结果和模型输出都不能添加配对或修改目标地址。

```toml
[[delegation_targets]]
node_id = "worker-example"
endpoint = "https://worker.example:7448"
capabilities = ["workspace.read"]
```

这段配置只声明允许的路由，不创建配对，也不授予 Worker 超出配对记录的能力。可用能力限于 `workspace.read` 与 `workspace.write.approval`；实际范围还受本次执行计划约束。只有允许分布式执行的计划才向 Harness 提供此工具，Worker 的子运行不继续委派。

模型通过 `call` 选择 `dispatch`、`get`、`reconcile`、`cancel` 或 `accept`。任务只能属于本次父 Run、会话、所有者与 `owner_epoch`。`dispatch` 要求目标、指令、能力和超时，daemon 为当前模型调用生成稳定的任务与尝试标识，避免重复调用产生第二次执行。

派发、取消和结果验收分别要求准确动作审批。派发审批绑定目标地址、完整配对指纹和任务范围；验收审批还绑定已核对结果的 SHA-256 摘要与原文。配对身份、会话所有权或结果变化后，旧审批失效。Worker 返回内容始终标为不可信，不能代替授权。

执行意图持久化后发生断线、取消或回执丢失，任务进入 `Uncertain`，本次运行停止并要求核对原任务。`reconcile` 查询同一任务身份，不重新派发。人工管理命令见[客户端](clients.md)。

`apps/agentd/tests/delegation.rs` 通过三个模型协议的真实适配器、本地模型 mock、Master 与配对 mTLS Worker 的 Harness 验证派发、审批和结果验收，并覆盖拒绝、撤销配对、身份变化、跨父运行访问、范围扩大及未知结果禁止重放。
