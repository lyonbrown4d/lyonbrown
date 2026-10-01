# 本地 gRPC C/S 架构

> 状态：TUI/GUI 迭代已接受
>
> 范围：本地 `agentd`、终端客户端与 GPUI 客户端
>
> 延后：移动端、公网 endpoint、Relay、OAuth 与远程控制

## 决策

Lyonbrown 把应用 Runtime 的所有权收敛到一个本地 `agentd`。目标架构中，TUI 与 GUI 都是同一套版本化 gRPC 契约的客户端，不再分别构建 Kernel。

```text
agent TUI ──┐
            ├── gRPC / HTTP/2 / loopback ── agentd ── Kernel
agent GUI ──┘                                  ├── 模型
                                               ├── 工具与 MCP
                                               ├── 工作区索引
                                               └── Session 存储
```

首个实现只绑定 loopback，不是远程控制 endpoint。迁移期间及测试场景保留 embedded Runtime，但它与 gRPC 路径使用同一套应用命令和快照语义。

## 协议形态

Protocol Buffers 是协议真相源。命令与查询使用 unary RPC；可合并的 Runtime 完整快照使用一条 server-streaming RPC。

| 形态 | 操作 |
|---|---|
| Unary command | 开始、引导、排队、取消、审批响应 |
| Unary query | 握手、健康检查、最新快照 |
| Server stream | 带 revision 的最新 Runtime 完整快照 |

每条命令包含 `command_id` 和 `client_instance_id`；每次投影更新包含单调递增的 `revision`。客户端重连后先取得 Snapshot，再从已应用的 revision 之后订阅。v1 的 stream 不是事件日志：慢客户端可能直接收到更新的完整快照，revision 可以跳号。客户端按 revision 去重，只应用更新快照；它不回放或猜测被合并的中间状态。

每条命令还携带握手得到的 `expected_server_instance_id`。daemon 在登记幂等键和执行命令之前原子校验该值；旧 daemon 已执行但响应丢失时，同一个命令不会被重放到重启后的新 daemon。外层事件 revision 必须与内层 Snapshot revision 一致，否则客户端把连接标记为协议失败。

daemon 使用有容量的进程内 replay cache，在一次逻辑调用的 transport 重试中复用同一个 `command_id`。这个保证只覆盖同一 daemon 实例且记录仍在 cache 中的场景；daemon 重启或记录淘汰后不提供跨进程 exactly-once。客户端不得在发现 `server_instance_id` 已变化后盲目重放未确认的非幂等命令。

Wire contract 与 Rust 领域类型分离。`Instant`、内部错误和实现细节枚举必须映射成稳定 protobuf 值。新增字段使用新 tag，禁止复用已经发布的 tag。

v1 的 gRPC 编解码预算统一为 16 MiB。超过预算的 Snapshot 明确失败；后续如果实际会话逼近此预算，应优先引入分页或有损历史裁剪，而不是继续无界提高上限。

## 所有权

- `agent-protocol`：protobuf 定义、生成类型与协议版本。
- `agent-client`：TUI/GUI 共用的连接、命令关联、Snapshot 获取、stream 订阅与重连策略。
- `agentd`：组合根、Kernel/Runtime 生命周期、模型加载与下载任务、工作区索引、MCP、审批和 Session 存储。
- TUI/GUI：只持有呈现状态、输入映射和 client 生命周期。

关闭客户端不会自动取消 turn 或模型下载；取消只能通过显式 RPC。daemon 仲裁多客户端：第一个有效响应解决审批，后到响应收到类型化冲突。

普通 TUI 命令在可取消后台任务中执行，不阻塞键盘事件循环；取消与强停使用独立并发 RPC 和短超时。daemon 收到关闭信号时会先终止 Snapshot stream，再等待 gRPC 优雅退出，因此在线客户端不会阻塞服务停止。

## 本地 endpoint 与生命周期

v1 使用 HTTP/2 gRPC 并仅监听 `127.0.0.1`，不得默认监听 `0.0.0.0`。自动启动 daemon 前还需补齐 endpoint discovery 与每次启动的本地认证；在此之前地址显式配置，只用于开发期本地连接。

loopback 不是身份认证。没有完成随机启动 token、仅当前用户可读的 discovery 文件和服务端 token 校验之前，不得把该 endpoint 设为默认产品路径，也不得允许它执行高权限工具。

目标启动顺序：

1. client 发现或启动 `agentd`；
2. client 完成版本与 capability 握手；
3. client 请求当前 Snapshot；
4. client 从该 Snapshot revision 开始订阅；
5. client 通过 unary RPC 发送幂等命令。

## 迁移顺序

1. 建立 protobuf、复用 Rust client 和可注入 daemon service，并完成离线测试。
2. 为 TUI 增加 transport adapter，同时保留 embedded mode。
3. 让 GPUI 状态模型接入同一个 client。
4. 把 CLI composition 与模型设置所有权迁入 `agentd`。
5. 增加 daemon discovery、认证、进程监管、重连与 revision 保留。

移动端和远程 transport 后续在此契约上扩展，本地迭代不会因此提前扩大安全边界。

## 当前落地状态

- 已完成：protobuf/tonic 基础契约、完整快照双向映射、daemon epoch 栅栏、取消安全的有限 replay cache、16 MiB 消息预算、loopback-only 可注入服务、优雅关闭，以及版本与 revision 校验。
- 已完成：TUI 统一客户端端口、embedded 兼容入口、非阻塞命令提交，以及负责握手、首个快照、完整快照 stream、有限重试和重连的远程 adapter。
- 已完成：GPUI 专用 native bridge、daemon epoch/连接状态模型，以及显式注入后的实时状态订阅；默认 GUI 仍不猜测 endpoint。
- 尚未启用：`agentd` 可执行进程、默认 daemon 启动、endpoint discovery、本地认证、模型与 composition 所有权迁移。
