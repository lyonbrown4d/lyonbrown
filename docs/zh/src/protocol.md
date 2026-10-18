# 协议与平台能力

公共协议位于 `proto/agent/v1/agent.proto`，包名为 `agent.v1`。Rust 类型由 Prost 和 Tonic 生成，再由 `agent-protocol` 提供给服务端与客户端。协议只包含版本化消息，不暴露模型库、Harness 或 UI 具体类型。

## 服务边界

| 服务 | 消息覆盖范围 |
| --- | --- |
| `ClientService` | 节点信息、会话、运行、事件、取消与审批 |
| `WorkspaceService` | 文件读取、修订校验写入、整工作区差异与监听 |
| `OrchestrationService` | 本地 Master 派发、分页查询、核对、取消与结果采纳 |
| `TaskService` | Worker 任务提交、查询、事件、取消与结果采纳 |
| `PeerService` | 节点信息、配对与撤销信任 |
| `BrowserService` | 浏览器 Space、Tab、观察、操作与人工接管 |
| `BrowserRuntimeService` | 客户端内嵌 Runtime 的认证双向挂接 |
| `ComputerRuntimeService` | 交互用户原生执行环境的反向消息契约 |
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

## 平台适配

工作区为浏览器、计算机操作、分布式连接和本地推理设置独立 crate。Windows、macOS、Linux 的原生依赖和可用能力应由对应适配层报告。统一协议不能消除系统 SDK、显示服务器、权限和真实设备差异。

本地推理的工程边界是 `mistral.rs` 与 `llama-cpp-2` 推理适配；推理库不接管 Session、Tools 或 Agent Loop。模型配置与决策权限见[模型与执行决策](models.md)，真实模型的独立运行方法见[开发与验证](development.md)。

BrowserRuntime 以 crate 嵌入客户端，CEF helper 通过客户端私有模式运行，浏览器执行与 GPUI UI 状态隔离。没有匹配的 CEF SDK 和运行资源时，浏览器能力不能被视为可用。Profile 的持久化与活页面恢复分开说明，避免把磁盘目录存在误认为网页状态已经恢复。

## 原生桌面执行边界

`agent-computer` 封装屏幕观察、输入和桌面租约；`agent-computer-runtime` 在进程内保留准备好的操作，并按 daemon Session、Run 和 `owner_epoch` 校验调用归属。执行消息只引用原准备记录、动作标识和内容摘要，不能替换输入内容。等待审批后，Runtime 使用原租约和原观察限制重新观察；内容变化、租约失效或取消都会阻止输入。准备记录在刷新或输入前消耗，失败后也不能重放同一动作。

不同 Runtime 必须共享桌面租约注册表。按令牌撤销时，注册表在同一锁内校验并取消准确令牌，过期控制者不能撤销替代租约。私有 postcard 消息限制为 68 MiB，以容纳最多 64 MiB 的 RGBA 图像和元数据；超长帧前缀在分配载荷前拒绝。持久审批、执行意图、进程重启后的去重、helper 身份验证与跨进程互斥由宿主负责，进程内记录不能代替这些保证。

这些组件与反向消息契约不构成客户端到 Harness 的原生控制验收。macOS 原生工厂在无法通过公开会话探测确认解锁状态时返回 `SessionStateUnknown`，不启动屏幕捕获或输入。Windows 与 Linux 适配同样需要对应平台的权限和实际运行验收。
