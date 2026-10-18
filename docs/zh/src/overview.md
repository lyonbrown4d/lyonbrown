# 项目概览

Lyonbrown 将 Agent 执行放在常驻的 `agentd` 进程中。终端客户端和 GPUI 桌面通过认证协议访问会话、任务和事件。客户端只负责输入、呈现和连接状态；关闭窗口不构成取消任务的指令。

本文档面向本地使用者和开发者，与当前 Rust 工作区对应。首次运行参阅[本地运行](quickstart.md)，理解执行和恢复机制参阅[会话、任务与恢复](persistence.md)。文档中的源码机制说明不代表真实模型、原生 SDK 或跨设备场景已经验收。

## 工作区

| 目录 | 职责 |
| --- | --- |
| `apps/agentd` | daemon 入口与服务装配 |
| `apps/agent-cli` | Ratatui 终端工作台 |
| `apps/agent-desktop` | GPUI 原生桌面 |
| `crates/agent-browser-runtime` | 嵌入 CLI/Desktop 的 CEF 执行环境与认证反向连接 |
| `apps/agent-control` | 控制面入口 |
| `crates/agent-client` | 客户端共用的认证连接 |
| `crates/agent-protocol`、`proto/agent/v1` | 版本化协议与类型转换 |
| `crates/agent-runtime` | `rpi-harness` 适配与会话恢复 |
| `crates/agent-storage` | SQLite 状态、事件和 Outbox |

其他能力按[协议与平台能力](protocol.md)说明其边界。目录或协议消息的存在不等于对应平台能力已经接通。

## 阅读图表

图表使用 Mermaid 绘制。点击图表可打开大图，滚轮缩放、拖拽移动，按 `Esc` 关闭。图下提供 Mermaid 源码和 SVG 下载；浏览器禁用脚本时仍可查看源码。页面主题使用 mdBook 原生主题选择器，图表颜色随主题更新。
