# Node.js / TypeScript + Pi 交付计划

> 状态：实施中。受支持的产品范围是本地 `agentd` daemon 与 `agent` 终端
> client。GUI/Desktop 和旧 Rust 实现不再是兼容目标。

## 产品边界

Lyonbrown 统一使用 Node.js 24 LTS、pnpm、ESM 与 strict TypeScript。

- `apps/agentd` 是唯一组合根，持有 Runtime、Pi 适配器、模型、工具、MCP、
  Secret、EventStore 与本地发现服务。
- `apps/agent-cli` 是认证 gRPC TUI client，只持有展示状态；运行 `agent` 时
  默认直接进入 chat。
- `apps/docs` 是中英文 VitePress 文档站。
- 不保留 embedded Runtime、Rust bridge、GUI client 或双写路径。

Pi 的具体类型必须停留在适配层，不得进入 protobuf 契约、领域事件或 UI model。
仍需保留的历史行为必须转化为 TypeScript 回归测试或语言无关 fixture。

## 目标包

- `packages/protocol`：protobuf 源、生成的 gRPC binding、wire 限制与兼容 fixture。
- `packages/agent-domain`：标识符、事件、不变量与纯 reducer。
- `packages/kernel-pi`：Pi Agent Core 与 Pi AI 适配器。
- `packages/agent-runtime`：持久化优先的编排与 Snapshot feed。
- `packages/agent-client`：认证 gRPC、重连/replay 与 epoch fencing。
- `packages/local-control-plane`：发现、生命周期与原生访问控制边界。
- `packages/control-plane-native`：安全的操作系统能力。
- `packages/event-store`：SQLite 存储、完整性与恢复。
- `packages/local-model`：托管 `node-llama-cpp`、显式下载、校验、原子激活与模型生命周期。
- `packages/ui-model`：与 Pi 无关的 TUI 状态和命令投影。

## 当前进度

| 阶段       | 状态   | 已交付 / 剩余工作                                                                    |
| ---------- | ------ | ------------------------------------------------------------------------------------ |
| P0 工程    | 已完成 | Node/pnpm 基线、strict ESM、统一门禁、双语 VitePress                                 |
| P1 协议    | 进行中 | protobuf/gRPC、认证 client、replay 与 wire 测试已存在；继续扩展黑盒 fixture          |
| P2 状态    | 进行中 | reducer、SQLite EventStore、恢复和 Windows 路径锚定已落地；补齐迁移/回滚演练         |
| P3 Runtime | 进行中 | Pi kernel 与 durable Runtime 适配器已落地；补齐工具、MCP、审批、上下文与 Secret 链路 |
| P4 Daemon  | 进行中 | server、control plane、launcher、本地模型基础已存在；补齐生产组合根和进程适配器      |
| P5 TUI     | 进行中 | 将 `apps/agent-cli` 迁移到 Pi TUI，接通真实 client，完成双语交互闭环                 |
| P6 发布    | 进行中 | Windows 可移动开发制品与 native smoke 已通过；仍需归档、签名、安装、升级/回滚验证    |

## TUI 里程碑

当前产品里程碑同时满足以下条件才算完成：

- 运行 `agent` 立即渲染 chat，不需要 `chat` 子命令；
- daemon 发现、启动、认证、重连和 stale epoch 状态不会阻塞首次渲染，并且可观察；
- 模型缺失不会强制下载；用户显式启动、取消或重试 daemon 后台任务并看到进度；
- 流式对话、取消、steering、follow-up 队列、审批、命令面板、Activity Inspector、
  模型选择和 Session 恢复全部通过 `packages/agent-client` 与 `packages/ui-model` 完成；
- 英文/简体中文、六套内置主题、窄终端、SSH/tmux、终端安全文本与纯键盘操作有测试；
- TUI 不读取 Secret、模型文件、EventStore 文件或原始 daemon token。

布局、命令、快捷键、主题与细化状态行为以
[TUI 视觉与交互设计基线](../design/tui-interaction-design.md)为准。

## 完成定义

- 干净 checkout 通过 frozen install、format、lint、typecheck、test、build 与文档门禁；
- 协议 fixture、恢复测试和 daemon/TUI 端到端场景覆盖取消、审批、重连、epoch 变化与恶意 payload；
- EventStore 与模型存储在受支持平台通过崩溃、竞态、DACL、reparse point、完整性与回滚测试；
- Windows x64/arm64 打包验证 native addon、daemon/TUI 同装、签名、升级、回滚和卸载；
- 运维诊断与恢复文档不会暴露路径中的 Secret、token、prompt 或模型供应商凭据。

## 主要风险

- `node-llama-cpp` ABI、prebuild、GPU、驱动与 CPU fallback 兼容性；
- Windows DACL、文件身份、reparse point、named pipe 与原子激活安全；
- Windows Terminal、SSH、tmux 与窄终端之间的能力差异；
- Pi 事件时序、取消、工具调用与上下文预算的语义漂移；
- EventStore schema 错误导致本地历史不可恢复。
