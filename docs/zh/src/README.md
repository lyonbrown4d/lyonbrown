# Lyonbrown 文档

<p><a href="../en/index.html">English</a></p>

Lyonbrown 是一个本地优先、模型供应商中立的通用 Agent Runtime。

## 阅读导航

- [Runtime 架构设计](architecture/local-first-agent-runtime.md)：长期架构目标、核心原则、模块边界与阶段划分。
- [本地 gRPC C/S 架构](architecture/grpc-client-server.md)：`agentd`、TUI/GUI client、认证、发现与作用域边界。
- [FluxDI 组合根](architecture/composition-root.md)：应用装配边界、依赖图验证与替换策略。
- [TUI 视觉与交互设计基线](design/tui-interaction-design.md)：单一对话流、中英文界面、六套内置主题、实时引导、内联审批和控制面板。
- [第一阶段 MVP 实施计划](plans/mvp-implementation-plan.md)：M0–M9 的交付内容、验收标准、风险与验证命令。
- [第二阶段产品与交互实施计划](plans/phase-two-product-plan.md)：模型控制面、TUI/GUI 与跨端交互契约。
- [配置参考](reference/configuration.md)：分层配置、远程模型、MCP、会话存储与 TUI。
- [本地模型与任务路由](reference/local-models-routing.md)：由 `agentd` 持有的默认 llama.cpp、模型缓存和类型化路由。

## 当前状态

- Lyonbrown 已统一为 C/S 产品架构：`agentd` 是 Kernel、模型、工具、MCP 与
  Session 存储的唯一所有者；TUI/GUI 都是经过认证的 gRPC client，不保留内嵌
  Runtime 兼容路径。
- llama.cpp 是 `agentd` 的默认能力。配置的模型缺失时，Runtime 进入
  `ModelSetupRequired`，不会强制下载；daemon 已支持显式后台下载、取消、进度与
  epoch 激活；TUI/GUI 现已通过同一 gRPC 状态提供双语显式设置操作项、进度、
  重试/取消和 fail-closed epoch fencing。
- `agent` 默认直接进入 Ratatui/Crossterm chat TUI，`agent .` 是对应的显式
  写法；不再保留旧的 chat/headless 子命令。
- TUI 对已识别的简体中文系统 locale 使用 `zh-CN`，否则使用 `en-US`。`F2` 全局
  切换语言；`/language`、`/language en-US` 和 `/language zh-CN` 都是 client 本地
  命令，不发送到 daemon，也不改变 transcript 或 Session state。
- 跨平台进程 sandbox 仍属于 Phase 2，尚未纳入本轮 MVP。

里程碑的详细范围和验收口径以 MVP 实施计划为准。架构文档描述长期方向，不直接表示当前实现进度。
