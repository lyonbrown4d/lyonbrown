# 本地 gRPC C/S 架构

> 状态：已接受并进入落地验证
>
> 范围：本地 `agentd`、终端客户端与 GPUI 客户端
>
> 延后：移动端、公网 endpoint、Relay、OAuth 与远程控制

## 决策

Lyonbrown 统一采用本地 Client/Server 架构。TUI 与 GUI 都是同一套版本化
gRPC 契约的客户端；客户端不再创建 Kernel，也不保留 embedded 产品模式或
未认证 transport 兼容入口。

```text
agent TUI ──┐
            ├── gRPC / HTTP/2 / loopback ── agentd ── Kernel
agent GUI ──┘                                  ├── llama-cpp 与其他模型
                                               ├── 工具与 MCP
                                               ├── 工作区索引
                                               └── Session 存储
```

`agentd` 是 Runtime、模型、工具、MCP、索引、审批与 Session 的唯一所有者。
TUI/GUI 只持有呈现状态、输入映射和 client 生命周期。这一边界也意味着关闭
某个客户端不会隐式取消正在运行的 turn 或模型任务；取消必须通过显式 RPC。

当前阶段只允许本机 loopback 通信，不把本地端口视为远程控制入口。

## 协议形态

Protocol Buffers 是协议真相源。命令与查询使用 unary RPC；可合并的 Runtime
完整快照使用一条 server-streaming RPC。

| 形态 | 操作 |
|---|---|
| Unary command | 开始、引导、排队、取消、审批响应 |
| Unary query | 握手、健康检查、最新快照 |
| Server stream | 带 revision 的最新 Runtime 完整快照 |

每条命令包含 `command_id`、`client_instance_id` 和握手返回的
`expected_server_instance_id`。命令进入 native client 队列前，调用方会把它绑定
到当前已接受的 `server_instance_id`；wire request 将该值保留为 expected epoch，
而不是在稍后发送时重新读取连接状态。daemon 在登记幂等键和执行命令之前原子
校验 server instance；不一致时返回类型化 `EpochMismatch` detail，client 将其
映射为 `StaleEpoch`。因此旧 epoch 命令会 fail closed，不会因响应丢失而跨入
replacement Runtime。

成功 receipt 只有在 `accepted = true`，且 `protocol_version` 与 `command_id` 均
与请求一致时才会被接受。拒绝或字段不匹配的 receipt 属于协议错误，不能被视为
命令已接受。

每次投影更新包含单调递增的 `revision`。客户端连接后先获取 Snapshot，再从
已应用 revision 之后订阅。v1 stream 不是事件日志：慢客户端可能直接收到更
新的完整快照，因此 revision 可以跳号。客户端只应用更新 revision，不推断被
合并的中间状态。事件外层 revision 与内层 Snapshot revision 不一致时视为协议
错误。

daemon 使用有容量的进程内 replay cache；一次逻辑调用的 transport 重试复用
同一个 `command_id`。该保证只覆盖同一 daemon 实例且记录尚未淘汰的情况，不
提供跨进程 exactly-once。发现 `server_instance_id` 变化后，客户端不得盲目重
放未确认的非幂等命令。

Wire contract 与 Rust 领域类型分离。新增 protobuf 字段只能使用新 tag，禁止
复用已发布 tag。v1 的 gRPC 编解码预算为 16 MiB；超限 Snapshot 明确失败，后
续应使用分页或有界历史裁剪，而不是无界提高上限。

## 本地认证与作用域

`agentd` 仅监听随机分配的 `127.0.0.1` 端口。loopback 只限制网络可达性，不
等同于身份认证，因此所有产品 RPC 都要求每次 daemon 启动随机生成的 bearer
token。公开的未认证 client/server API 已删除；未携带或携带错误 token 的请求
在进入业务 service 前统一拒绝。

discovery descriptor 包含以下最小信息：

- loopback 地址、PID 与 `server_instance_id`；
- 每次启动生成的 bearer token；
- workspace 和 Runtime 配置共同派生的 `scope_fingerprint`；
- schema version 与 transport 类型。

客户端与 daemon 分别计算 Runtime 配置 fingerprint。workspace 文件系统身份与
配置 fingerprint 一起派生 scope，因此等价的路径写法不会分裂出第二个 daemon；
daemon 启动时校验配置 fingerprint，握手再返回
scope fingerprint。客户端只有在 discovery、握手与预期 scope 三者一致时才复
用该实例，避免连接到其他工作区或不同配置的 daemon。

## Runtime epoch 切换

`agentd` 内部使用 in-process epoch supervisor 切换 Runtime。模型安装完成后，
supervisor 会在旧 control plane 仍可服务时构建新 Runtime；构建成功后先启动新
loopback endpoint，再原子发布新的 discovery descriptor，最后才优雅停止旧
endpoint 和旧 Runtime。显式的 graceful shutdown 之后仍有 abort cleanup，避免
异常任务跨 epoch 泄漏。

每次切换都会轮换 endpoint 地址、bearer token 与 `server_instance_id`，但
workspace scope、daemon PID 和 daemon 全生命周期持有的 `agentd.lock` 保持不变。
client 会重新执行 discovery、认证握手和 Snapshot resync，不把旧 epoch 的未确认
命令盲目重放到新实例。

若新 Runtime 构建或新 endpoint 启动失败，supervisor 保留仍在线的旧 control
plane，发布可恢复失败并允许显式重试；只有在新 descriptor 成功发布后才允许旧
endpoint 退出。这是本地 control plane 的 rollback/fail-closed 边界。

## 发现、复用与启动

客户端入口统一经过 launch coordinator：

1. 读取并严格校验 owner-only discovery descriptor；
2. 使用 descriptor 中的 token 发起带超时的 gRPC 握手；
3. 实例 ID、scope 与配置匹配时复用现有 daemon；
4. 无可复用实例时竞争 startup lock，胜者启动 `agentd`；
5. 等待 discovery 发布并完成认证握手，其他客户端随后复用同一实例；
6. 获取当前 Snapshot 并开始订阅，再发送命令。

该流程由 native client driver 在后台执行；TUI 与 GUI 会立即呈现“连接中”状态，
不会因 daemon 启动而阻塞界面。daemon 重启后，重连会重新执行 discovery，使新
endpoint、bearer token 与 server epoch 同时替换旧的命令和事件通道。

启动协调和 daemon 所有权使用两把不同的锁：

- `agentd.start.lock` 只用于客户端启动选主，在 daemon 就绪后释放；
- `agentd.lock` 由 daemon 在整个生命周期持有，确保一个 scope 只有一个 owner。

拆分锁可以避免客户端等待 readiness 时阻塞 daemon 获取生命周期锁。锁和
discovery 文件位于同一个安全运行目录；descriptor 使用原子发布，token 不通
过命令行参数或环境变量传递。

Unix 已实现 owner-only 运行目录、文件权限、原子 discovery 发布和进程锁。
Windows 通过受保护的 current-user-only DACL 提供等价边界，拒绝 reparse point，
并在使用前校验 descriptor 与 lock 的句柄。两个平台都不会退化为宽松权限，也
不会启动未认证 daemon。

## 所有权与 Rust 生态选型

- `agent-protocol`：protobuf 定义、生成类型与协议版本；
- `agent-client`：认证连接、命令关联、Snapshot、stream、重试与重连；
- `agent-local`：token、discovery、安全目录与双锁；
- `agent-launch`：discover/reuse/spawn/readiness 协调；
- `agent-runtime`：配置、composition、模型与 Runtime 服务；
- `agentd`：唯一 composition root 和 gRPC server；
- TUI/GUI：仅负责交互与呈现。

实现优先复用成熟 Rust 生态：`tonic`/`prost` 承载 gRPC 与 protobuf，`tokio`
承载异步 Runtime、进程与信号，`serde` 负责受限 schema，`getrandom`、`secrecy`
和 `subtle` 用于 token 生成、秘密包装与常量时间比较，`file-id` 提供跨平台
workspace 身份，Unix 文件安全边界使用 `rustix`。只有项目特有的 scope、
生命周期与 UI 语义保留在自有 crate 中。

## 模型与下载边界

`llama-cpp` 是 `agentd` 的默认能力和默认 feature，不属于 TUI/GUI。模型缺失
不会触发启动期下载：daemon 返回可操作的模型状态，客户端仍可进入界面并选择
配置或显式开始后台下载。默认托管 preset 是 `qwen3-1.7b-q4_k_m`；用户显式配置
的 `model_path` 缺失时不能改为托管下载，只能修复配置或选择其他模型。

下载控制面使用 unary `StartModelDownload` / `CancelModelDownload`，所有进度和
结果继续进入唯一的版本化 `RuntimeSnapshot` stream，不增加第二条模型事件流。
托管模型状态包含来源、phase、允许动作、字节进度、failure 和 `operation_id`。
phase 覆盖等待锁、检查、下载、校验、安装、激活、就绪、取消和失败；失败后的
动作称为 Retry，而不是 resume。

下载任务属于 daemon：关闭 TUI/GUI 不会取消任务。Cancel 必须携带当前
`operation_id`，旧操作 ID 不能取消较新的下载。进入 `Installing` 后取消返回
TooLate，因为此时原子落盘不能被中断。模型 ID、URL、摘要和目标路径均由 daemon
内置目录决定，client 不能提交任意 URL 或文件路径。

## 当前落地边界

协议、认证、scope 握手、安全 discovery、双锁、launch coordinator、`agentd`
composition、可重连共用 client 以及 TUI/GUI 统一 C/S 入口已经落地，并由
workspace 测试覆盖。显式后台模型下载、单 Snapshot 进度流、in-process epoch
切换和 fail-closed 命令 epoch fencing 也已落地。

两端现已在同一组 Snapshot/RPC 上提供完整模型设置闭环：显式开始或重试、进度、
允许时取消、失败详情留屏，以及英文/简体中文文案。GUI 将设置卡片放在
conversation 与 Composer 之间，Runtime 状态栏显示真实 phase/progress，顶部栏
提供中英文切换。TUI 根据系统 locale 选择启动语言，并在 setup、approval 和
conversation 中提供全局 `F2` 切换；`/language` 由 client 本地处理，不发送 daemon
命令，也不改变 transcript/Session state。两端在 stale 状态下继续显示诊断信息，
但在收到已接受 epoch 的 fresh Snapshot 前禁用模型设置操作；两端都不实现下载器。
TUI 现已使用同一份双语十命令目录驱动 slash 解析、`Ctrl-P` 命令面板和帮助；
未实现项明确禁用，未知 slash 输入由本地拦截，模型设置/审批交互优先于浮层。
已接受的 Snapshot 现在也会投影为只读 TUI 活动检查器。`Ctrl-O` 打开以稳定
turn/step ID 定位的详情；已接受的新 revision 更新投影与缓存，而重复 revision 或
epoch 不匹配的 Snapshot 不能改变它。断线时保留最后一次已接受的证据并标为 stale
缓存；Session 或 daemon epoch 改变时关闭检查器并清除旧选择。详情生成带缓存，并对
字段、集合、JSON、行数和总字符数设置预算上限。

这个 MVP 以中英文展示路由与 fallback 原因、上下文与 token 用量、工具参数/结果/
耗时、outcome 和相关 ID。它不是完整 event trace：尚无时间戳、严格事件历史、历史
`ToolPreview`/capabilities 或完整 diagnostics；剪贴板复制也未实现。后续前端工作包括
M12 的真实会话/模型选择器、主题浮层与偏好持久化，以及 M13 的完整 GUI transcript、
流式输出、审批、取消与恢复闭环。M12 尚未完成。

移动端与远程控制延后。未来可以扩展 transport 和认证体系，但不得复用本地
discovery token 或在未建立远程身份、授权与加密边界前暴露当前 loopback 服务。
