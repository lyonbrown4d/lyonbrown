# Lyonbrown 文档

<p><a href="../en/index.html">English</a></p>

Lyonbrown 是一个本地优先、模型供应商中立的通用 Agent Runtime。

## 阅读导航

- [Runtime 架构设计](architecture/local-first-agent-runtime.md)：长期架构目标、核心原则、模块边界与阶段划分。
- [FluxDI 组合根](architecture/composition-root.md)：应用装配边界、依赖图验证与替换策略。
- [第一阶段 MVP 实施计划](plans/mvp-implementation-plan.md)：M0–M9 的交付内容、验收标准、风险与验证命令。
- [配置参考](reference/configuration.md)：分层配置、远程模型、MCP、会话存储与 TUI。
- [本地模型与任务路由](reference/local-models-routing.md)：默认 llama.cpp、Qwen3 自动选择、模型缓存和类型化路由。

## 当前状态

- 已完成：M0–M9 的 MVP 实施与本机验收，包括 Secrets、远程 Provider 与回退、MCP、SQLite 会话恢复，以及默认启用的进程内 llama.cpp。
- `agent .` 使用 Ratatui/Crossterm TUI；`agent ask`、`agent chat`、`agent sessions` 和 `agent resume` 提供 headless/会话入口。
- 跨平台进程 sandbox 仍属于 Phase 2，尚未纳入本轮 MVP。

里程碑的详细范围和验收口径以 MVP 实施计划为准。架构文档描述长期方向，不直接表示当前实现进度。
