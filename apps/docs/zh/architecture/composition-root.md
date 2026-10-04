# TypeScript 组合根

Lyonbrown 的唯一生产组合根位于 Node/TypeScript `agentd`。领域包通过构造函数、
工厂和由 consumer 定义的窄接口表达依赖，不使用 service locator，也不把依赖注入
容器扩散到领域模型。TUI 与 GUI 是 gRPC 客户端，不再构造 Kernel 或 Runtime。

## 装配边界

启动按以下顺序完成：

1. 解析并冻结 Runtime 配置、工作区身份与路径策略；
2. 获取安全运行目录和 daemon 生命周期锁；
3. 打开 EventStore，并从持久化事件恢复 Runtime 投影；
4. 构造模型、工具、审批与 Pi Kernel 适配器；
5. 构造 `agent-runtime`，再启动认证的 loopback gRPC server；
6. 原子发布 discovery descriptor，最后进入 ready 状态。

```text
配置 + EpochIdentity + 安全本地能力
                 │
                 ▼
            agentd composition
                 │
                 ├── EventStore
                 ├── Pi Kernel adapter
                 ├── Model / Tool / Approval ports
                 ├── AgentRuntime
                 └── authenticated gRPC server
```

组合使用显式工厂而非容器：

- `createAgentRuntime({ store, kernelFactory, clock, ids })`
- `createAgentGrpcServer({ host, token, serverInstanceId, scopeId })`
- `createAgentClient({ binding, deadlines })`

Pi、SQLite 与 gRPC 的具体类型分别留在各自适配包内。领域事件和 Runtime 投影不依赖
这些实现类型。生产入口不导出任意解析器、未认证 server、embedded runtime 或测试
bootstrap。

## 生命周期与失败边界

`agentd` 独占模型、工具、Session 和持久化状态。命令只有在持久化成功后才可发布
Snapshot 或成功 receipt；无法确认副作用结果时，当前 Session 进入终止状态，不能
继续推测执行。

`EpochIdentity` 将 `server_instance_id`、scope 与 bearer token 作为一次启动的不可变
整体注入。替换 Runtime 时创建新的组合实例并轮换 epoch，而不是热替换全局单例。

安全 native provider 是生产组合根的硬依赖。缺失时入口必须以
`NATIVE_SECURITY_PROVIDER_REQUIRED` fail closed；不得退化为普通路径文件操作、
`proper-lockfile`、原始 endpoint/token 参数或 Rust bridge。

## 测试替换策略

测试在 consumer 边界注入内存 EventStore、fake Kernel/model、fake clock/ID 和 fake
control-plane provider。gRPC 测试仍使用真实 `grpc-js` loopback transport，验证认证、
epoch fencing、replay 与 stream 行为；fake 不作为产品入口导出。

当前迁移阶段允许旧 Rust 代码作为行为 oracle 留在分支中，但它不属于最终运行时，
也不形成兼容模式。对应 TypeScript 垂直切片通过对照测试后，旧实现将整体移除。
