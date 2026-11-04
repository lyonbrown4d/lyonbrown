# 协议与平台能力

公共协议位于 `proto/agent/v1/agent.proto`，包名为 `agent.v1`。Rust 类型由 Prost 和 Tonic 生成，再由 `agent-protocol` 提供给服务端与客户端。协议只包含版本化消息，不暴露模型库、Runtime 或 UI 具体类型。

## 服务边界

| 服务 | 消息覆盖范围 |
| --- | --- |
| `ClientService` | 节点信息、会话、运行、事件、取消与审批 |
| `WorkflowService` | 强类型计划、步骤、宿主验证、变更预览和人工撤销 |
| `WorkspaceService` | 文件读取、修订校验写入、整工作区差异与监听 |
| `OrchestrationService` | 本地 Master 派发、分页查询、核对、取消与结果采纳 |
| `TaskService` | Worker 任务提交、查询、事件、取消与结果采纳 |
| `PeerService` | 节点信息、配对与撤销信任 |
| `BrowserService` | 浏览器 Space、Tab、观察、操作与人工接管 |
| `BrowserRuntimeService` | 客户端内嵌 Runtime 的认证双向挂接 |
| `ComputerRuntimeService` | 绑定 daemon 会话的认证原生执行环境连接 |
| `ControlService` | 注册、解析、配对与双向转发消息 |

此表列出协议契约，不是可用功能清单。服务端未装配的服务或平台适配必须明确返回不可用，不能用成功的占位结果表示支持。

`agent-client` 为请求加入 `authorization: Bearer …` 元数据。地址和令牌由客户端配置提供。默认地址为本机 HTTP；公网使用前必须另外核对传输保护、节点身份、授权与部署配置，不能把本机 token 验证等同于公网部署验收。

直连与出站 Relay 的身份核验、配对权限、配置和故障恢复见[连接节点与 Control Relay](network.md)。

## 关键字段

| 字段 | 语义 |
| --- | --- |
| `session_id` | 持久会话标识 |
| `owner_node_id` / `owner_epoch` | 会话或任务的写入所有者与代次 |
| `idempotency_key` | 同一逻辑提交的去重标识 |
| `attempt_id` | 一次 Worker 执行尝试，区别于 Task 本身 |
| `cursor` / `after_cursor` | 事件位置与增量读取起点 |
| `deadline_ms` / `expires_at_ms` | 任务截止时间与审批过期时间，毫秒 |

## 强类型执行事件与运行中输入

`agent_core::events::AgentEvent` 是应用拥有的执行事实枚举。`PhaseChanged` 使用 `ExecutionPhase` 和可选 `ModelIdentity`；工具开始／结束、用量、队列状态和 Run 终态分别有明确字段。任意工具参数仍使用 JSON 值，类型属于应用协议，不包含 Runtime、Provider SDK 或 UI 类型。TUI 与 Desktop 使用共享解码入口匹配枚举，不从持久化审计 JSON 推断阶段。

`Event` 使用 Run ID、游标、时间戳和 `kind/payload_json` 信封；执行事实使用 `kind=agent_event` 和带 `type` 标签的序列化枚举。当前 daemon 的 Run 状态事件由共享解码器转换；Runtime 审计事件不推断为执行事实。未知类型只保留名称，已知类型的错误载荷返回解析错误。`model_request_started`、`model_response_committed` 等 Runtime 变更仅提供白名单审计元数据，不作为客户端阶段协议。Runtime 在发起模型请求前产生 `ConnectingModel`；该阶段不证明 TCP 连接成功。

`SubmitRunInput` 要求 Run ID、owner epoch、UUID request ID、显式 `STEER/FOLLOW_UP` 类型和非空提示（最多 1 MiB）。队列确认与 Runtime 快照、事件、Outbox 同事务提交；同 request ID、同内容返回原回执，改变内容或目标 Run 会拒绝。单 Session 不能同时启动另一个 Run。`ListRunInputs` 按指定 Run 所属 Session 返回输入记录，包含跨 Run 保留的待处理输入，并保留每条回执的原始 Run 归属。快照保留全部待处理项与最多 100 条终态记录，`history_truncated` 标记历史裁剪；单条提示预览最多 4096 字节，`prompt_truncated` 明示裁剪，不能把预览重新当作完整请求。待处理输入最多 100 条、合计 1 MiB。`CancelRunInput` 校验原条目和当前 Session epoch，路由当前绑定的 Runtime；已消费的条目不会伪装成已撤销。执行边界后的 Pending 不自动唤醒新运行，断线也不重放提交。没有挂接 Runtime 的重启后队列只提供核对，不声明即时撤销已恢复。

工具、用量和队列事件在 runtime 适配层从 SDK 强类型记录转换后，与快照和 Outbox 原子写入；daemon 绑定明确的 Run ID，不按“最后创建的 Run”猜归属。撤销先持久记录 `CancelRequested` 意图，再调用 Runtime；只有持久取消或已提交的消费事实才能结清。失败会关闭输入、停止执行并将 Run 置为 `uncertain`，即使原回复已完成也保留原结果供核对。未结清意图拒绝新的 StartRun，启动恢复同样重建这道限制，不自动重放队列。能力通过 `events.semantic` 与 `runs.input` 声明，旧 daemon 不支持时不回退为普通 StartRun。

## 平台适配

工作区为浏览器、计算机操作、分布式连接和本地推理设置独立 crate。Windows、macOS、Linux 的原生依赖和可用能力应由对应适配层报告。统一协议不能消除系统 SDK、显示服务器、权限和真实设备差异。

本地推理的工程边界是 `mistral.rs` 与 `llama-cpp-2` 推理适配；推理库不接管 Session、Tools 或 Agent Loop。模型配置与决策权限见[模型与执行决策](models.md)，真实模型的独立运行方法见[开发与验证](development.md)。

BrowserRuntime 以 crate 嵌入客户端，CEF helper 通过客户端私有模式运行，浏览器执行与 GPUI UI 状态隔离。没有匹配的 CEF SDK 和运行资源时，浏览器能力不能被视为可用。Profile 的持久化与活页面恢复分开说明，避免把磁盘目录存在误认为网页状态已经恢复。

## 原生桌面执行边界

CLI 与 Desktop 以默认关闭的 `native-computer` feature 转发启用 `agent-computer-runtime/native`。原生 SDK 通过显式 `native` feature 启用：`agent-computer-runtime/native` 同时启用 `agent-computer/native`，并提供 `attach_native` 工厂。默认构建保留协议、执行引擎和可注入的 `attach_host`，不引入截图与输入 SDK。daemon 使用这条默认依赖路径。

原生目标发现只读取元数据，不捕获像素或请求系统授权。发现前后均检查交互会话与已有屏幕权限；最小化和零尺寸目标被排除，目录超过 512 项则拒绝。目标标签去除控制字符并限制为 512 个字符；`DesktopTarget` 的 Debug 输出省略可能含私密内容的标签。窗口描述符保留进程 ID，挂接后按窗口 ID 与进程 ID 核对，拒绝将替换窗口当成原目标。标题与几何信息按新观察核对，不把旧目录当成当前画面。

`agent-computer` 封装屏幕观察、输入和桌面租约；`agent-computer-runtime` 在进程内保留准备好的操作，并按 daemon Session、Run 和 `owner_epoch` 校验调用归属。执行消息只引用原准备记录、动作标识和内容摘要，不能替换输入内容。等待审批后，Runtime 使用原租约和原观察限制重新观察；内容变化、租约失效或取消都会阻止输入。准备记录在刷新或输入前消耗，失败后也不能重放同一动作。

客户端库可通过 `attach_native` 将原生宿主嵌入客户端进程并连接 daemon。连接前先核对 daemon 节点及选定会话的归属；握手携带固定的桌面范围，daemon 再次校验身份，并记录会话的 `owner_epoch`。每个会话只允许一个宿主连接，模型工具固定本次连接代次，每次操作均在发送前和接收后核对 Run 状态及会话归属；重新连接不能替换运行已经选定的宿主。连接成功本身不授予截图或输入权限。

连接只允许一个待返回命令；同一连接代次的请求串行排队，续租不能抢占已派发命令。等待可取消，超过 35 秒则拒绝，派发前再次校验连接代次。daemon 按请求标识、宿主、连接代次、Session、`owner_epoch`、私有协议版本和预期响应类型校验返回值，并检查租约、观察范围与图像、窗口、AX 节点边界。非法响应或断线会取消连接，不自动重放输入。原生调用和消息编解码在阻塞线程执行。已经进入操作系统的调用仍需实际结束；取消不会强行终止线程。原生宿主使用用户数据目录中的文件锁，按操作系统图形会话互斥，改变 daemon 节点标识或客户端数据目录不能绕过这把锁。执行线程持有文件锁至调用结束。

同进程的原生 Runtime 共享桌面租约注册表。按令牌撤销时，注册表在同一锁内校验并取消准确令牌，过期控制者不能撤销替代租约。私有 postcard 协议版本为 2，包含准确令牌的 `RenewLease`。续租只延长当前未撤销、未过期的令牌，不能取得新令牌或恢复旧控制权。消息总上限为 68 MiB，以容纳最多 64 MiB 的 RGBA 图像和元数据；超长帧前缀在分配载荷前拒绝。daemon 将能力响应限制为 4 KiB、观察响应限制为 68 MiB、其他响应与请求限制为 16 KiB。持久审批、执行意图和进程重启后的去重仍由 daemon 管理，进程内记录不能代替持久记录。

daemon 已通过这条认证连接装配 Runtime 的 `computer_observe` 与 `computer_act`，启用条件、准确审批和持久回执见[本地工具与浏览器操作](tools.md#模型计算机工具)。CLI/TUI 与 Desktop 已提供显式目标选择和会话连接入口，见[原生计算机连接](clients.md#原生计算机连接)；配置开关本身不能建立原生宿主。这些组件不构成客户端到 Runtime 的原生控制验收。macOS 会话检查与限制见下文。Windows 与 Linux 适配同样需要对应平台的权限；两平台的实际运行验收尚未完成。

macOS 通过 CoreGraphics 读取调用进程的会话，要求位于控制台、登录完成，且 `CGSSessionUniqueSessionUUID` 能解析为非全零 UUID。检查还通过维护中的 `objc2` 绑定读取 IORegistry 根节点，要求 `IOConsoleLocked` 明确为 `false`；它与 `CGSSessionScreenIsLocked` 任一为 `true` 时返回 `Locked`。`IOConsoleLocked` 缺失或类型错误、会话 UUID 缺失或无效时返回 `SessionStateUnknown`。会话检查前后各读取一次快照并校验；快照变化时拒绝。锁字段属于 macOS 的实际实现字段，以上为项目采用的兼容策略，不是 Apple 文档承诺的锁屏状态保证。

原生后端一旦观察到锁屏、无法读取的会话状态或会话身份变化，就保持撤销状态；解锁后同一后端也不会恢复控制，必须重新显式挂接。会话检查、屏幕捕获和输入仍是分开的操作系统调用，前后校验不构成原子性的捕获与输入保证。实现见 `crates/agent-computer/src/native/macos.rs`、`session.rs` 与 `mod.rs`。

## Provider Profile 控制接口

认证 ClientService 提供 `ListProviderProfiles`、`PutProviderProfile`、`RemoveProviderProfile` 与 `ProbeProviderProfile`，通过 `providers.profiles` 能力标识开放。协议使用 `RemoteProviderProtocol` 枚举，只接入 Responses、Chat Completions 和 Anthropic Messages。Profile DTO 包含 UUID、名称、协议、Base URL、模型 ID、修订号和凭据存在标记，不返回凭据值或内部存储引用。

客户端为新表单生成稳定 UUID，`revision=0` 表示创建；编辑提交已读取的修订号，daemon 在同一 SQLite 事务内校验并递增版本、保存配置、事件和 Outbox。移除要求 `expected_revision`，已移除身份由保留事件阻止复用。错误使用既有 gRPC 状态：校验失败为 `InvalidArgument`，版本冲突为 `Aborted`，缺失为 `NotFound`，容量限制为 `ResourceExhausted`。结果丢失后先查询原 UUID，不自动生成新身份重放。

新建必须携带单行、最多 16 KiB 的凭据；更新时空字节数组表示保留已有值。凭据由加密存储持有，修改时创建不可变引用，存储 CAS 失败不会改写旧引用；未能确认发布状态的孤立凭据保留，不盲目清除。Profile 配置与加密凭据文件不构成跨存储原子事务。删除 Profile 不撤销已固定的 Session/Run 模型绑定，旧凭据没有自动回收。

Probe 对未保存草稿执行校验和模型列表 GET，不保存配置或执行推理。只有使用已保存凭据时才要求匹配当前修订号。HTTPS 地址或回环 HTTP 地址须不含嵌入凭据、查询参数和 fragment；探测禁用代理、重定向与自动重试，最长 5 秒，响应最多 1 MiB、100 个合法模型 ID。失败响应不回显上游正文、地址中的秘密或密钥。此限制与具体推理 SDK 的真实环境验收分别记录。

`ClientService` 另提供上下文统计、显式摘要查看、压缩与撤销、会话模式和图片 Run 提交。计划、步骤、模式、变更和重建状态均使用枚举；图片消息的调试输出不包含正文。`WorkflowService` 单条变更最多 12 MiB 传输包，客户端图片输入包上限 40 MiB，业务图片总量限制仍为 32 MiB。机制与使用限制见[计划、上下文与变更](agent-workflows.md)。
