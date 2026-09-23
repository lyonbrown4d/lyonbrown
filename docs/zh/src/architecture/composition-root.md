# FluxDI 组合根

Lyonbrown 使用固定版本 `FluxDI 1.2.2` 作为仓库级装配骨架。它只负责连接对象，不把依赖注入扩散到业务模型：`agent-core`、`agent-model`、`agent-tool`、`agent-secret`、`agent-profile`、`agent-storage` 等领域 crate 继续通过构造函数、trait 和显式参数表达依赖，不依赖 FluxDI。CLI 与 GPUI GUI 各自拥有独立组合根；未来的 server 或其他应用入口也应采用相同边界，而不是让领域 crate 感知容器。

## 装配边界

启动分成两个阶段：

1. `CoreModule` 绑定已经确定的 `RuntimeConfig`、`RuntimePaths` 和 `ApprovalPolicy`。
2. 异步准备外部资源后，`RuntimeModule` 绑定模型与工具注册表、Secret Broker、Agent/Workspace Profile、Skill Registry、可选 Storage、可选 Task Router，以及最终的 `KernelFactory`。

GUI 的 `GuiModule` 则绑定 `CockpitState` 与 `CockpitFactory`，在创建窗口前完成图验证。两个应用只公开 `build_kernel()` 或 `create_cockpit()` 这类类型化入口，不公开通用容器解析器。

```text
配置 + 路径 + 审批策略
          │
          ▼
   FluxDI CompositionRoot
          │
          ├── ModelRegistry / TaskRouter
          ├── ToolRegistry / SecretBroker
          ├── AgentProfile / WorkspaceProfile / SkillRegistry
          ├── SessionStore（可选）
          └── KernelFactory ──► Kernel
```

`KernelFactory` 是容器与领域构建器之间的窄适配层。它从 Profile 生成 `RoutingPolicy`，把配置、注册表、审批、Secret、Profile、存储和路由器传给 `Kernel::builder()`。Kernel 本身不解析配置，也不从全局容器取依赖。

## 依赖图验证

组合根在异步服务全部注册后执行 `try_validate_graph()`。该验证检查缺少的绑定、循环依赖和不完整的 `KernelFactory` 依赖，并在 CLI 开始处理会话前失败。所有新增根级服务都应：

- 在应用模块中注册；
- 在 Provider 上用 `with_dependency` 声明依赖；
- 通过 `CompositionRoot::validate()` 和组合根测试证明图可解析。

运行时仍可能因模型文件、网络或数据库打开失败而启动失败；图验证只验证装配关系，不替代资源健康检查。

## 覆盖与替换策略

替换从组合根的输入边界完成，而不是在领域 crate 中使用 service locator：

- 配置按“内置默认值 → 全局配置 → 已信任的工作区配置 → `--config` → 环境变量 → CLI 参数”覆盖，再把唯一的 `RuntimeConfig` 实例注入图中。
- 路径与审批策略由调用方传入，因此测试可使用临时目录和 `SessionApprovalPolicy`，无需修改领域代码。
- Provider、工具、Secret Broker、Profile、Storage 和 Task Router 先在 `prepare_services` 中构造，再作为接口或注册表绑定；替换实现应在这一应用层 seam 完成。
- 可选能力使用显式包装类型绑定，避免“未注册”和“配置为关闭”混为一谈。

生产组合根不应在启动后热替换单例。测试若需要不同实现，应创建独立组合根；新应用入口应复用领域构建 API，但拥有自己的模块与生命周期。
