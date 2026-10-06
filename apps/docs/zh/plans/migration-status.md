# 迁移进度

> 已验证检查点：2026-10-06
>
> 后续开发直接在 `main` 上继续。本页会区分“已经实现并有测试覆盖”和“发布验证”，
> 不表示产品已经可以发布。

## 已合入 `main`

- Node.js 24、固定版本 pnpm、strict ESM、仓库门禁与双语 VitePress 文档。
- TypeScript domain、protocol/gRPC、认证 client、launch coordinator、durable
  Runtime 适配器、Pi kernel 适配器、SQLite EventStore、本地 control plane、
  Windows 原生路径校验和本地模型基础。
- Node TUI 已替换原 Rust `apps/agent-cli`，并精确固定
  `@earendil-works/pi-tui@1.0.0`。
- TUI 默认直接进入 chat，包含即时连接状态、Snapshot revision fencing、中文与
  英文、六套主题、模型设置、审批、命令面板、Activity Inspector 和终端文本安全。
- GUI/Desktop、根 Cargo 元数据、Rust crates/providers、Rust 构建脚本和最后的
  Rust TUI 快照均已删除。受跟踪仓库中不再包含产品 `.rs`、`Cargo.toml` 或
  `Cargo.lock` 文件。
- Windows 原生模型存储已经覆盖私有 ACL、reparse point 与文件身份校验、安装锁、
  有界 staging、原子激活和 installed-file anchoring。
- staging 契约现已支持真实 ipull 生命周期：先写 `filename.ipull`，重命名为最终
  staging 文件，安全重新锚定，并在下载失败后清理固定名称的残留。
- Protocol `.proto` schema 与版本化 wire fixture 继续作为受跟踪的契约源；生成的
  TypeScript/gRPC 文件由 Git 忽略，并在 lint、typecheck、test、build 前由包生命周期
  自动生成。
- 认证 loopback gRPC channel 显式关闭环境 HTTP proxy 发现，避免本地 daemon 流量被
  重定向到代理。
- 生产 launcher 与 `agentd` composition root 已接通。daemon 会打开 SQLite
  EventStore，持有 Pi Runtime 与本地模型服务，在认证 gRPC 就绪后才发布发现信息，
  并按所有权逆序进行有界关闭。
- 本地模型 lease 已通过 `node-llama-cpp` 提供有界的流式 chat。默认只允许一个活跃
  inference，使用固定 4096-token context，传递取消信号，并限制请求、输出、队列与
  关闭资源。
- 模型发现与下载仍在后台执行。下载是可选的，只能由用户显式启动；TUI 会投影设置、
  进度、取消、失败、激活与激活重试状态。
- client 可见 Snapshot 使用 daemon 自有的单调 transport revision，Runtime 恢复
  revision 与仅模型状态的更新不会破坏 client fencing。
- 每条 user、follow-up 或 steering 消息在 daemon 命令入口限制为 64 KiB，与
  local-model 的单消息上限一致。Kernel 的 256 KiB 是构造后 inference 总输入的
  防御上限，不是单条消息配额。非活跃 Session 会在达到 32 个历史 turn 或 4 MiB
  用户文本后，于下一次输入前轮换；活跃 turn 的 follow-up 限制为 8 条、合计
  512 KiB。daemon 通过同一条 FIFO 命令通道串行执行预算检查与持久化，避免并发 RPC
  绕过限制。

## 当前检查点的验证证据

- 包级回归测试覆盖 TUI、Windows 原生模型存储、protocol 冷生成，并会拒绝受 Git
  跟踪的 protocol 生成输出。
- ipull 回归测试在真实 Windows 文件系统上执行 rename，然后再封存下载文件；另一个
  失败测试验证遗留 `.ipull` 与 staging 目录都会被删除。
- 聚焦的 TypeScript 测试已经覆盖生产组合、model-aware Runtime 激活与重试、
  transport revision、本地推理并发与清理、Session 轮换和重启恢复。
- 当前聚焦测试数量为：`agentd` 176、`agent-cli` 28、`ui-model` 56、
  `local-model` 86。
- 构建后的生产 launcher/独立 `agentd` 启动黑盒已于 2026-10-06 通过：
  `source=spawned`、认证 health 为 `serving`、模型设置为 `required`、
  `allowedActions=[download]`、投影 revision 为 `2`。全程没有自动生成 GGUF，结束后
  临时 daemon 与目录均已清理。
- 重启测试会刻意轮换任何包含历史 turn 的 Session，包括已完成的 Session。这样既
  保留持久事件，也不会在 Pi 对话状态尚未完整恢复时静默续接；Pi history hydration
  仍未实现。
- 本检查点已通过 frozen install、仓库级 format、lint、typecheck、test、build、docs
  和 `ci-gate`；在声明可发布前仍须基于最终发布制品重复执行。

## 当前产品状态

仓库已经只保留 Node.js / TypeScript 实现。生产 C/S 路径已从 TUI launcher 经认证
gRPC 接到 daemon 持有的 EventStore、Pi Runtime 和本地模型。这是“已实现且有聚焦
测试”的检查点，仍不是发布证据。

生产 Kernel 当前刻意使用空 tool registry、默认拒绝审批，并拒绝 Secret 解析；MCP
尚未接入。GUI、Desktop 和移动端仍不在当前范围。构建产物启动黑盒已经验证，但该
黑盒没有下载或运行模型。真实 GGUF 下载、激活、推理和取消，以及发布打包仍未验证。

架构保持唯一：`agentd` 持有 Runtime、模型、工具、MCP 与 EventStore，TUI 只是认证
gRPC 展示 client。不得增加 embedded Runtime、GUI fallback 或 Rust 兼容路径。

## 后续顺序

1. 在支持的发布硬件上，用固定真实 GGUF 验证下载、激活、取消、一次流式 turn 与
   有界关闭。
2. 将构建产物认证黑盒从已验证的 spawn 与 setup-required 检查点扩展到复用、重启
   fencing、模型进度、推理与取消。
3. 在允许 daemon 重启后续接历史 Session 前实现 Pi history hydration。
4. 逐步增加窄化的工具、审批执行、MCP、上下文与 Secret；各能力独立验证前继续
   default deny。
5. 完成可分发打包，再执行 frozen install、完整 format、lint、typecheck、test、
   build、docs 与 `ci-gate`。
