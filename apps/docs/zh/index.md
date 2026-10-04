# Lyonbrown 文档

<p><a href="../en/index.html">English</a></p>

Lyonbrown 是一个本地优先、模型供应商中立的通用 Agent Runtime。

## 阅读导航

- [本地 gRPC C/S 架构](architecture/grpc-client-server.md)：`agentd`、TUI client、认证、发现与作用域边界。
- [FluxDI 组合根](architecture/composition-root.md)：应用装配边界、依赖图验证与替换策略。
- [TUI 视觉与交互设计基线](design/tui-interaction-design.md)：单一对话流、中英文界面、六套内置主题、实时引导、内联审批和控制面板。
- [Node.js / TypeScript + Pi 迁移计划](plans/node-typescript-pi-migration.md)：目标架构、TUI 里程碑、完成定义与切换风险。
- [迁移暂停检查点](plans/migration-status.md)：已合入内容、WIP 分支、已知阻断与恢复顺序。
- [配置参考](reference/configuration.md)：分层配置、远程模型、MCP、会话存储与 TUI。
- [本地模型与任务路由](reference/local-models-routing.md)：由 `agentd` 持有的默认 llama.cpp、模型缓存和类型化路由。

## 当前状态

迁移现已暂停。目标仍是唯一 C/S 架构：`agentd` 持有 Runtime 状态，TUI 是认证
展示 client；GUI/Desktop 不在范围内，也没有 embedded fallback。

当前已合入分支还不是可端到端运行的产品。Node TUI 与 Windows 原生模型存储改动
保存在功能分支，存在已记录的阻断且尚未合入。准确 commit、验证证据、blocker 与
恢复顺序见[迁移暂停检查点](plans/migration-status.md)。
