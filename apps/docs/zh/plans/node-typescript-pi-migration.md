# Node.js / TypeScript + Pi 迁移计划

> 状态：迁移方向已确定，实施仍在进行中。本文不表示行为对等或切换已经完成。

## 目标与不变量

Lyonbrown 将从当前 Rust 实现迁移到 Node.js 24 LTS、pnpm、ESM 与 strict TypeScript workspace。迁移只替换实现技术，不削弱本地优先产品契约。

不可妥协的不变量如下：

- `EventStore` 仍是 session、turn、step、审批和可恢复执行状态的真相源。Pi 是执行引擎，不是事实存储。
- `apps/agentd` 是唯一组合根，持有 runtime、模型、工具、MCP 连接、secret 与存储。`apps/agent` 和 `apps/desktop` 都是经过认证的 client，不嵌入或拥有 runtime。
- Pi 具体类型必须封装在适配层，不得泄漏进 protobuf/gRPC 公共契约、领域事件或 UI model。
- 在证明 TypeScript 行为对等之前，现有 Rust 实现及其测试始终是行为 oracle。

## 目标架构

### 应用

- `apps/agentd`：常驻本地 daemon 与组合根，承载 gRPC、kernel、Pi 适配器、EventStore、工具执行、模型设置和本地控制面。
- `apps/agent`：基于 Pi TUI（`@earendil-works/pi-tui`）和 Lyonbrown UI model 的终端 client，通过 gRPC 与 `agentd` 通信。
- `apps/desktop`：Electron + React 桌面 client，与 TUI 共用 client 和 UI-model 契约，不承载 runtime。
- `apps/docs`：双语 VitePress 文档站。

### 包

- `packages/protocol`：protobuf 源、生成的 gRPC binding、wire DTO、版本策略与兼容性 fixture。
- `packages/agent-domain`：领域事件、标识符、不变量与纯状态转换。
- `packages/kernel-pi`：Pi Agent Core / Pi AI 的隔离适配层。
- `packages/agent-runtime`：持久化优先的编排、恢复与 Snapshot feed。
- `packages/agent-client`：认证 gRPC client、重连/replay 与 client 侧 epoch fencing。
- `packages/local-control-plane`：发现、endpoint 发布、进程生命周期与操作系统访问控制适配器。
- `packages/event-store`：EventStore、migration、幂等与恢复。
- `packages/tooling`：工具/MCP 适配、策略执行、审批与审计元数据。
- `packages/ui-model`：TUI/desktop 共用且与 Pi 无关的 view model、projection、command 与交互契约。
- `packages/local-model`：`node-llama-cpp` 集成、模型缓存/设置、下载生命周期与激活 fencing。
- `packages/i18n-theme`：双语文案、locale 选择、主题与展示偏好持久化。

### 技术映射

- Pi Agent Core（`@earendil-works/pi-agent-core`）通过 kernel 适配器执行 agent loop。
- Pi AI（`@earendil-works/pi-ai`）通过 Lyonbrown model port 提供模型供应商集成。
- Pi TUI（`@earendil-works/pi-tui`）为 `apps/agent` 提供终端渲染原语。
- `node-llama-cpp` 提供默认本地推理适配器。
- `@grpc/grpc-js` 与生成的 protobuf binding 保持 daemon/client 边界。
- Electron + React 实现桌面 shell 和 UI，并消费 `packages/agent-client` 与 `packages/ui-model`。

## 必须保持的行为

迁移必须保留 Rust 测试与兼容性 fixture 表达的可观察行为：

- protobuf field number、可选性、状态/错误映射与前后向兼容；
- runtime/model epoch、过期命令 fail-closed 拒绝与激活 fencing；
- 重连 replay 顺序、cursor、幂等、snapshot 与事件恢复；
- 本地发现、认证 gRPC session、授权 scope 与凭据轮换；
- 工具审批请求、决定、取消、过期、可审计性与默认拒绝；
- secret 脱敏、受保护存储、最小权限，以及事件/日志不持久化 secret；
- prompt、telemetry、崩溃数据、模型下载和 MCP 流量的隐私边界；
- context 装配、压缩、预算、稳定顺序与重启连续性；
- skill 发现、校验、优先级、执行与错误报告；
- MCP 连接生命周期、能力协商、工具/resource 映射、取消与恢复；
- 显式模型设置状态、进度、取消、重试、校验、epoch 激活与离线行为。

## 分阶段交付

### P0 — 工程基线与文档

建立 Node 24.15.0、pnpm 10.33.4、ESM、strict TypeScript、lint/type/test/build 门禁、VitePress、架构边界和冻结 lockfile。本阶段不迁移 runtime 行为。

### P1 — Protocol 与对等测试框架

建立 `packages/protocol`，确定性生成 TypeScript gRPC binding，导入 protobuf golden fixture，并建立可对 Rust/TypeScript endpoint 运行相同场景的黑盒框架。先冻结 wire 兼容性，再迁移 runtime 行为。

### P2 — Domain、EventStore 与 Storage

将标识符、事件、reducer、snapshot、migration、replay 和崩溃恢复迁入 `packages/agent-domain`、`packages/event-store`。用 Rust fixture 比较事件流和 projection，包括损坏尾部与重复投递场景。

### P3 — Kernel 与 Pi 执行适配器

实现 `packages/kernel-pi` 与 `packages/agent-runtime`，适配 Pi Agent Core 和 Pi AI 且不暴露 Pi 类型。迁移 context、skill、tool、MCP、approval、secret/privacy、取消与错误语义；为每类执行场景验证确定性领域事件。

### P4 — `agentd`、gRPC 与本地模型

建立 daemon 组合根、认证 gRPC 服务、发现/控制面与 `node-llama-cpp` 模型设置。证明 epoch fencing、replay、重启恢复、权限和 Windows service/process 行为。

### P5 — TUI 与桌面 Client

用 Pi TUI 构建 `apps/agent`，用 Electron + React 构建 `apps/desktop`。两者共用 protocol、client、UI-model、国际化和主题包；证明 client 不拥有 runtime 状态，且重连/replay 产生等价 UI projection。

### P6 — 对等、打包与切换

运行完整黑盒 parity suite、迁移/恢复测试、长时间 soak、Windows installer、升级/回滚演练和签名制品校验。只有满足完成定义后，才可移除 Rust，并以该迁移分支覆盖主分支。

## 完成定义

同时满足以下条件，迁移才算完成：

- 从干净 checkout 使用 frozen lockfile 时，根 pnpm 门禁及所有 workspace package 门禁全部通过；
- VitePress 能构建两个语言版本，内部链接和资源有效；
- protocol golden test 与 Rust/TypeScript 黑盒 parity test 覆盖以上全部行为，且不存在未解释差异；
- EventStore 升级、replay、崩溃恢复、幂等和回滚演练在真实迁移数据上通过；
- 安全评审覆盖 auth、approval、secret、privacy、DACL、MCP/tool 执行、依赖审计与 Electron 隔离；
- Windows x64/arm64 的 native addon、daemon、TUI、desktop、installer、签名、升级与卸载在支持环境通过；
- 运维文档、诊断、数据导出、降级策略与恢复手册完整；
- 发布批准明确记录 Rust 已不再需要作为行为 oracle。

只有在上述条件与 parity 全部满足后，才在可评审的切换变更中移除 Rust，并按仓库治理流程以该迁移分支覆盖主分支。在此之前必须保留 Rust 代码和测试。

## 主要风险

- **Windows native addon：**`node-llama-cpp` 受 Node ABI、MSVC runtime、GPU backend、CPU 架构和驱动兼容性影响；需要明确验证 prebuild、源码构建 fallback、checksum 和 CPU fallback。
- **DACL 与本地传输：**named pipe、发现文件、模型/cache 路径和 secret 材料都需要每用户 DACL；安装、升级、service account 变化和恢复后必须重新验证，失败时保持 fail-closed。
- **Electron 打包：**ASAR 布局、native module 解包、Electron ABI rebuild、代码签名、SmartScreen、自动更新原子性和回滚可能与开发环境不同。
- **语义漂移：**Pi 的事件时序和取消行为可能与 Rust 不同，必须由领域适配器和 golden event trace 吸收差异，不能改变公共行为。
- **数据兼容：**EventStore schema 错误可能让本地历史不可恢复；前向 migration、备份、回滚边界和基于 fixture 的恢复测试都是发布阻断项。
