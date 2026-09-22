# Lyonbrown 文档

<p><a href="../en/index.html">English</a></p>

Lyonbrown 是一个本地优先、模型供应商中立的通用 Agent Runtime。

## 阅读导航

- [Runtime 架构设计](architecture/local-first-agent-runtime.md)：长期架构目标、核心原则、模块边界与阶段划分。
- [第一阶段 MVP 实施计划](plans/mvp-implementation-plan.md)：M0–M9 的交付内容、验收标准、风险与验证命令。
- [配置参考](reference/configuration.md)：分层配置、环境变量、TUI 与内嵌 llama.cpp。

## 当前状态

- 已完成：M0–M4（核心、Provider、Tool Runtime、Profile、Workspace、Skills 与 Prompt Compiler）
- 已提前交付：基于 Ratatui/Crossterm 的 `agent .` TUI
- 已提前交付：feature-gated 的进程内 embedded llama.cpp provider 与 CLI 配置入口；真实 GGUF smoke 仍是 M9 验收项
- 下一阶段：M5 — Secrets
- M5–M8 的其余工作待实施；跨平台进程 sandbox 在 Phase 2 实施

里程碑的详细范围和验收口径以 MVP 实施计划为准。架构文档描述长期方向，不直接表示当前实现进度。
