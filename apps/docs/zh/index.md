# Lyonbrown 文档

<p><a href="../en/index.html">English</a></p>

Lyonbrown 是一个本地优先、模型供应商中立的通用 Agent Runtime。

## 阅读导航

- [本地 gRPC C/S 架构](architecture/grpc-client-server.md)：`agentd`、TUI client、认证、发现与作用域边界。
- [FluxDI 组合根](architecture/composition-root.md)：应用装配边界、依赖图验证与替换策略。
- [TUI 视觉与交互设计基线](design/tui-interaction-design.md)：单一对话流、中英文界面、六套内置主题、实时引导、内联审批和控制面板。
- [Node.js / TypeScript + Pi 迁移计划](plans/node-typescript-pi-migration.md)：目标架构、TUI 里程碑、完成定义与切换风险。
- [迁移进度](plans/migration-status.md)：已合入内容、验证证据与后续顺序。
- [配置参考](reference/configuration.md)：分层配置、远程模型、MCP、会话存储与 TUI。
- [本地模型与任务路由](reference/local-models-routing.md)：由 `agentd` 持有的默认 llama.cpp、模型缓存和类型化路由。

## 当前状态

迁移已经在 `main` 上恢复。仓库只保留 Node.js / TypeScript：Node TUI 与修正后的
Windows 原生模型存储已经合入；GUI、Desktop、embedded Runtime fallback 与 Rust
兼容路径仍不在范围内。

产品目前还不是端到端可运行检查点。下一步是具体 launcher adapters、完整的
`agentd` 生产组合和认证本地 daemon/TUI 黑盒流程。验证证据与实现顺序见
[迁移进度](plans/migration-status.md)。
