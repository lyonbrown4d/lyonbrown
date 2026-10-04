# Lyonbrown 文档

<p><a href="../en/index.html">English</a></p>

Lyonbrown 是一个本地优先、模型供应商中立的通用 Agent Runtime。

## 阅读导航

- [本地 gRPC C/S 架构](architecture/grpc-client-server.md)：`agentd`、TUI client、认证、发现与作用域边界。
- [FluxDI 组合根](architecture/composition-root.md)：应用装配边界、依赖图验证与替换策略。
- [TUI 视觉与交互设计基线](design/tui-interaction-design.md)：单一对话流、中英文界面、六套内置主题、实时引导、内联审批和控制面板。
- [Node.js / TypeScript + Pi 迁移计划](plans/node-typescript-pi-migration.md)：目标架构、TUI 里程碑、完成定义与切换风险。
- [配置参考](reference/configuration.md)：分层配置、远程模型、MCP、会话存储与 TUI。
- [本地模型与任务路由](reference/local-models-routing.md)：由 `agentd` 持有的默认 llama.cpp、模型缓存和类型化路由。

## 当前状态

- Lyonbrown 已统一为 C/S 产品架构：`agentd` 是 Kernel、模型、工具、MCP 与
  Session 存储的唯一所有者；TUI 是经过认证的 gRPC client，不保留内嵌
  Runtime 兼容路径。
- llama.cpp 是 `agentd` 的默认能力。配置的模型缺失时，Runtime 进入
  `ModelSetupRequired`，不会强制下载；daemon 已支持显式后台下载、取消、进度与
  epoch 激活；TUI 通过同一 gRPC 状态提供双语显式设置操作项、进度、
  重试/取消和 fail-closed epoch fencing。
- `agent` 默认直接进入 Node.js Pi TUI chat，`agent .` 是对应的显式
  写法；不再保留旧的 chat/headless 子命令。
- TUI 对已识别的简体中文系统 locale 使用 `zh-CN`，否则使用 `en-US`。`F2` 全局
  切换语言；`/language`、`/language en-US` 和 `/language zh-CN` 都是 client 本地
  命令，不发送到 daemon，也不改变 transcript 或 Session state。
- TUI 命令/帮助切片已提供中英文版本：`Ctrl-P`、输入框为空时的 `?` 与 `/help`
  共用一份固定的十命令目录，并明确禁用未完成项。双语只读活动检查器也已基于
  已接受的 Runtime Snapshot 落地：`Ctrl-O` 打开稳定的 turn/step 详情，保留并标记
  stale 缓存证据，并适配 30×8、52×12、90×20 三档终端。真实的会话/模型选择器和
  带偏好持久化的主题浮层仍属于 M12。
- GUI/Desktop 与旧 Rust 实现已经退出产品范围。仍需保留的历史行为转换为
  TypeScript 回归测试或语言无关的协议 fixture。
