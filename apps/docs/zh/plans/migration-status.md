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
- 模型下载使用 Node 24 内建 `fetch` 流式写入原生存储提供的同文件系统 staging；请求
  与写入共用取消信号，并在收到响应 headers 前执行有限次数的瞬时故障重试。
- Protocol `.proto` schema 与版本化 wire fixture 继续作为受跟踪的契约源；生成的
  TypeScript/gRPC 文件由 Git 忽略，并在 lint、typecheck、test、build 前由包生命周期
  自动生成。
- 认证 loopback gRPC channel 显式关闭环境 HTTP proxy 发现，避免本地 daemon 流量被
  重定向到代理。生产 composition 仅将 `HTTP_PROXY`、`HTTPS_PROXY`、`NO_PROXY` 及其
  小写形式传给 Node 24 `setGlobalProxyFromEnv`，供 daemon 持有的外部模型请求使用，
  不会把其他环境变量交给全局代理配置。
- 生产 launcher 与 `agentd` composition root 已接通。daemon 会打开 SQLite
  EventStore，持有 Pi Runtime 与本地模型服务，在认证 gRPC 就绪后才发布发现信息，
  并按所有权逆序进行有界关闭。
- 本地模型 lease 已通过 `node-llama-cpp` 提供有界的流式 chat。默认只允许一个活跃
  inference，使用固定 4096-token context，传递取消信号，并限制请求、输出、队列与
  关闭资源。Qwen3 的 `node-llama-cpp` 调用固定使用 `budgets.thoughtTokens=0`，防止
  隐藏 thought 消耗完对 client 可见的输出预算。
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
- 下载回归测试覆盖内建 `fetch` 的流式写入、响应 headers 前有限重试、取消传播、长度
  与字节上限校验，以及失败后的 staging 清理。
- 聚焦的 TypeScript 测试已经覆盖生产组合、model-aware Runtime 激活与重试、
  transport revision、本地推理并发与清理、Session 轮换和重启恢复。
- 当前聚焦测试数量为：`agentd` 180、`agent-cli` 28、`ui-model` 56、
  `local-model` 103、`control-plane-native` 85。
- 构建后的生产 launcher/独立 `agentd` 启动黑盒已于 2026-10-06 通过：
  `source=spawned`、认证 health 为 `serving`、模型设置为 `required`、
  `allowedActions=[download]`、投影 revision 为 `2`。全程没有自动生成 GGUF，结束后
  临时 daemon 与目录均已清理。
- Windows 开发机上的生产 composition 真实 GGUF smoke 已于 2026-10-06 通过。它对
  固定的 `Qwen3-1.7B-Q4_K_M.gguf`（1,107,408,544 bytes，SHA-256
  `228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f`）验证了：启动时
  不自动下载；显式启动首个 operation、出现进度后取消；用新的 operation 完整下载、
  校验并激活；生成 client 可见的短回答与 usage；取消长 turn 后继续完成下一 turn；
  使用同一缓存重启时不重新下载，并创建新 Session 与空 turns；活跃 inference 在
  15 秒界限内关闭。脚本最终输出 `REAL_GGUF_SMOKE_OK`，并清理临时模型缓存。
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
尚未接入。GUI、Desktop 和移动端仍不在当前范围。真实 GGUF 下载、激活、推理、取消、
缓存重启与有界关闭已经在当前 Windows 开发机的生产 composition 上通过；这不是发布
打包或最终 release artifact 的验证，不能据此声明产品可发布。

架构保持唯一：`agentd` 持有 Runtime、模型、工具、MCP 与 EventStore，TUI 只是认证
gRPC 展示 client。不得增加 embedded Runtime、GUI fallback 或 Rust 兼容路径。

## 后续顺序

1. 完成可分发打包，并基于最终 release artifact 在支持的发布硬件上重复固定真实
   GGUF 的下载、激活、取消、推理、缓存重启与有界关闭验证。
2. 将构建产物认证黑盒从已验证的 spawn 与 setup-required 检查点扩展到复用、重启
   fencing、模型进度、推理与取消。
3. 在允许 daemon 重启后续接历史 Session 前实现 Pi history hydration。
4. 逐步增加窄化的工具、审批执行、MCP、上下文与 Secret；各能力独立验证前继续
   default deny。
5. 对最终制品执行 frozen install、完整 format、lint、typecheck、test、build、docs
   与 `ci-gate`。
