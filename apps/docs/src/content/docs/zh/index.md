---
title: "Lyonbrown 文档"
---

<p><a href="/en/">English</a></p>

Lyonbrown 是一个本地优先、模型供应商中立的通用 Agent Runtime，产品主线采用 Node.js / TypeScript。

## 阅读导航

- [系统架构总览](/zh/architecture/overview/)：组件职责、Root/Subagent、跨节点任务和可选 Center。
- [本地 gRPC C/S 架构](/zh/architecture/grpc-client-server/)：`agentd`、TUI client、认证、发现与作用域边界。
- [组合根](/zh/architecture/composition-root/)：daemon 的能力所有权、启动与恢复边界。
- [Electron Desktop](/zh/architecture/electron-desktop/)：复用现有 daemon 的客户端骨架。
- [System One 决策层](/zh/architecture/system-one-decision-layer/)：OpenJev 前置决策、生命周期、安全边界与启用门槛。
- [TUI 视觉与交互设计基线](/zh/design/tui-interaction-design/)：单一对话流、中英文界面、六套内置主题、实时引导、内联审批和控制面板。
- [TUI 本机真实场景黑盒验收](/zh/design/tui-black-box-validation/)：记录真实 daemon、模型下载、推理、交互流程与生产阻塞项。
- [迭代路线](/zh/plans/roadmap/)：验证证据、单机可用性缺口与分布式主线。
- [配置参考](/zh/reference/configuration/)：分层配置、远程模型、MCP、会话存储与 TUI。
- [本地模型与任务路由](/zh/reference/local-models-routing/)：由 `agentd` 持有的默认 llama.cpp、模型缓存和类型化路由。

## 运行方式

TUI 与 Electron Desktop 通过认证 gRPC 连接 `agentd`。daemon 持有 Runtime、工具、模型与 EventStore；客户端负责输入、审批和状态展示。

操作能力面向 Windows、macOS 和 Linux。跨平台操作、单机任务、本地多 Agent、跨节点协作和可选 Center 的交付顺序与验收记录见[迭代路线](/zh/plans/roadmap/)。
