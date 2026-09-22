# Lyonbrown 文档

<p><a href="../en/index.html">English</a></p>

Lyonbrown 是一个本地优先、模型供应商中立的通用 Agent Runtime。

## 阅读导航

- [Runtime 架构设计](architecture/local-first-agent-runtime.md)：长期架构目标、核心原则、模块边界与阶段划分。
- [第一阶段 MVP 实施计划](plans/mvp-implementation-plan.md)：M0–M9 的交付内容、验收标准、风险与验证命令。

## 当前状态

- 已完成：M0 — Skeleton + agent-core
- 已完成：M1 — Kernel + MockProvider（离线 E2E）
- 已完成：M0–M3（含 Tool Runtime、审批与 capability-relative 文件隔离）
- 下一阶段：M4 — Profile + Workspace + Skills + Prompt Compiler
- M4–M8 待实施；M9 为可选的内嵌 llama.cpp stretch goal；跨平台进程 sandbox 在 Phase 2 实施

里程碑的详细范围和验收口径以 MVP 实施计划为准。架构文档描述长期方向，不直接表示当前实现进度。
