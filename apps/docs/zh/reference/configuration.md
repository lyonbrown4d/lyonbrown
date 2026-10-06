# 配置参考

> 状态：Node/TypeScript 配置加载器尚未接入生产入口。本文记录已接受的边界和待落地契约，不表示示例配置当前可运行。

## 配置所有权

配置只在 `agentd` 解析并冻结。TUI 可以提交经过协议定义的用户选择，但不读取 Provider 密钥、不决定存储路径，也不构造独立 Runtime。daemon 将规范化后的有效配置计算为 `effective_config_fingerprint`；它与工作区文件系统身份共同决定本地 daemon scope。

同一个 scope 内的 discovery、启动锁、生命周期锁、EventStore 和 Runtime 必须使用同一份冻结配置。配置变化需要建立新的受认证 epoch，不能原地改变旧 Runtime 的安全边界。

## 入口行为

`agent` 默认进入工作区 chat TUI，`agent .` 是显式写法。不会恢复 `chat`、`ask`、`sessions` 或 `resume` 兼容子命令。TUI 通过认证 gRPC 连接 `agentd`；只有 daemon 创建 Kernel 并持有 Runtime 状态。

客户端启动不能被模型检查或下载阻塞。daemon 不可用时显示连接状态；模型缺失时显示设置状态。只有显式用户操作才能开始后台模型下载。

## 已接受的优先级与信任规则

Node 配置加载器落地时必须采用确定性的优先级，并在计算 fingerprint 前完成 schema 校验和规范化：

1. 内置安全默认值；
2. 当前用户配置；
3. 显式指定的配置文件；
4. 经过白名单映射的 `LYONBROWN_` 环境变量；
5. 显式 CLI 覆盖。

工作区配置不是默认信任源。Provider endpoint、模型路由、MCP 命令、审批策略、存储路径和远程发送策略必须分别授权，不能用一个宽泛开关隐式放行全部能力。未知字段、NUL、相对安全根、不可规范化路径和冲突设置均 fail closed。

配置解析器、schema 库和格式仍属于待决选型；在实现与迁移工具完成前，不承诺旧版多格式兼容。

## 默认本地模型

默认 Provider 是 daemon 内的 llama.cpp；模型由 daemon 从固定可信目录中按本机硬件
推荐，不再写死单一默认 preset。`node-llama-cpp` 是必需依赖，但只允许使用随制品验证
的预编译 native binary，不允许运行时源码构建 fallback。

模型缺失不会触发下载。daemon 在后台检查安全缓存并投影 `ModelSetupRequired`；用户可以显式开始、重试或取消下载。URL、摘要和目标路径来自 daemon 内置目录，不能由 client 配置。详见[本地模型与任务路由](local-models-routing.md)。

## Secret、远程 Provider 与 MCP

密钥只能通过 SecretRef 引用，在实际发出远程请求时解析；不得写入 discovery、日志、Snapshot、事件数据库或命令行参数。远程 Provider 默认关闭，自动 fallback 只能用于明确允许的 Public 数据，不能发送 Secret、Confidential 内容或工具结果。

MCP server 命令由 daemon 持有并使用独立信任授权。工作区提供的可执行路径、参数和环境必须先审阅；工具调用仍通过统一的风险分类、审批、超时、取消和输出脱敏链路。

## 存储与路径

EventStore 使用 SQLite WAL，并由 daemon 独占。最终路径由工作区身份和配置 scope 派生，不接受 client 传入任意数据库路径。Windows 上必须由 native provider 验证 current-user owner/DACL、拒绝 reparse point，并在数据库及 WAL/SHM 生命周期内保持稳定句柄；缺少安全 capability 时不启动生产 daemon。

事件只有在数据库提交后才算接受。恢复时，已开始但没有持久化结果的工具调用标记为 outcome unknown，不能自动重放。

## 当前发布阻断项

- 版本化 Node 配置 schema、规范化与 fingerprint 实现；
- 用户配置目录和工作区配置的独立信任 UX；
- SecretRef 后端与日志/错误脱敏验证；
- native 安全模型缓存与稳定文件锚点；
- 配置迁移、诊断和双语错误消息。

在这些能力完成前，`apps/agentd` 的生产入口保持 fail closed，不使用硬编码 fingerprint 冒充可发布配置。
