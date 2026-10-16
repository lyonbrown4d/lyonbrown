# Lyonbrown 文档

<p><a href="../en/index.html">English</a></p>

Lyonbrown 是一个本地优先、模型供应商中立的通用 Agent Runtime。

## 阅读导航

- [本地 gRPC C/S 架构](architecture/grpc-client-server.md)：`agentd`、TUI client、认证、发现与作用域边界。
- [组合根](architecture/composition-root.md)：daemon 的能力所有权、启动与恢复边界。
- [Electron Desktop](architecture/electron-desktop.md)：复用现有 daemon 的客户端骨架。
- [System One 决策层](architecture/system-one-decision-layer.md)：OpenJev 前置决策、生命周期、安全边界与启用门槛。
- [TUI 视觉与交互设计基线](design/tui-interaction-design.md)：单一对话流、中英文界面、六套内置主题、实时引导、内联审批和控制面板。
- [TUI 本机真实场景黑盒验收](design/tui-black-box-validation.md)：记录真实 daemon、模型下载、推理、交互流程与生产阻塞项。
- [Node.js / TypeScript + Pi 迁移计划](plans/node-typescript-pi-migration.md)：目标架构、TUI 里程碑、完成定义与切换风险。
- [迁移进度与迭代路线](plans/migration-status.md)：验证证据、单机可用性缺口与分布式主线。
- [配置参考](reference/configuration.md)：分层配置、远程模型、MCP、会话存储与 TUI。
- [本地模型与任务路由](reference/local-models-routing.md)：由 `agentd` 持有的默认 llama.cpp、模型缓存和类型化路由。

## 当前状态

仓库只保留 Node.js / TypeScript。TUI 与新的 Electron Desktop 客户端骨架复用认证
gRPC 和 `agentd`；daemon 持有 Runtime、工具、模型与 EventStore。

Windows 开发机已有独立 daemon 与真实模型流程的验证记录，尚不能据此声明日常代码
任务可用或正式可发布。当前工作区已补充 POSIX 原生能力、搜索和进程工具，并通过 Mac 独立 daemon 验收；三平台完整任务、模型质量和上下文管理仍待验证或补齐。后续路线先按操作能力完成 Windows、macOS、Linux 跨平台升级与单机真实任务验收，再推进本地多 Agent、跨节点执行及可选中心。
具体证据和验收顺序见 [迁移进度与迭代路线](plans/migration-status.md)。
