# 迁移进度

> 已验证检查点：2026-10-07
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
  英文、六套主题、与设计稿一致的响应式模型设置界面、审批、命令面板、Activity
  Inspector 和终端文本安全。
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
- 手写 TypeScript 已统一迁移到 `@lyonbrown/...` workspace 根别名；测试门禁会拒绝
  相对模块引用和跨 workspace 私有别名访问，生产构建会将私有别名改写为可直接执行的
  Node ESM `.js` 路径。
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
- 模型发现与下载仍在后台执行。daemon 会使用固定可信目录与 `node-llama-cpp` 的
  RAM/VRAM、GGUF 元数据评估推荐适配硬件的模型；评估失败时保守选择目录中最小模型。
  下载是可选的，只能由用户显式启动；TUI 会投影推荐原因、设置、进度、取消、失败、
  激活与激活重试状态。
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
- 当前本轮聚焦测试数量为：`agentd` 208、`local-model` 135、脚本编排 48；完整仓库门禁
  仍以本页末尾列出的命令为准。
- 构建后的生产 launcher/独立 `agentd` 启动黑盒已于 2026-10-06 通过：
  `source=spawned`、认证 health 为 `serving`、模型设置为 `required`、
  `allowedActions=[download]`、投影 revision 为 `2`。全程没有自动生成 GGUF，结束后
  临时 daemon 与目录均已清理。
- Windows 开发机上的生产 composition 真实 GGUF smoke 已于 2026-10-07 通过。脚本从
  可信目录读取实际硬件选择及固定摘要，验证了不自动下载、显式取消/重试、进度、校验、
  激活、pinned/auto 选择、真实回答和 usage、`write` 调用、审批与准确落盘、长 turn 取消、
  缓存重启以及 30 秒预算内关闭。脚本最终输出 `REAL_GGUF_SMOKE_OK`，并清理临时缓存。
- daemon 重启现在会为已完成且无歧义的 Session 重建 Pi 历史，包括工具调用、工具结果
  与会话级审批策略。运行中、失败、取消、含 steering、待审批、未决结果或超过
  128 条消息/256 KiB 的历史仍会 fail closed 并轮换，持久事件继续保留。
- 本检查点已通过 frozen install、仓库级 format、lint、typecheck、test、build、docs
  和 `ci-gate`；在声明可发布前仍须基于最终发布制品重复执行。
- Windows x64 开发制品现已使用 pnpm hoisted production deploy，内嵌固定的 Node 24
  runtime，并提供相对路径的 `agent.cmd`。制品 smoke 会加载 CLI、公开 daemon entry、
  SQLite prebuild 与 llama.cpp CPU prebuild，同时禁止编译/下载 fallback。制品不含文件
  系统链接，并在目录改名后再次通过。这是打包证据，尚不是归档、签名、安装器或真实
  GGUF 的最终发布证据。

## 当前产品状态

仓库已经只保留 Node.js / TypeScript 实现。生产 C/S 路径已从 TUI launcher 经认证
gRPC 接到 daemon 持有的 EventStore、Pi Runtime 和本地模型。这是“已实现且有聚焦
测试”的检查点，仍不是发布证据。

生产 Kernel 当前使用受工作区约束的文件工具和显式审批，仍拒绝 Secret 解析；MCP
尚未接入。GUI、Desktop 和移动端仍不在当前范围。真实 GGUF 下载、激活、推理、工具、取消、
缓存重启与有界关闭已经在当前 Windows 开发机的生产 composition 上通过；可移动开发
目录已经验证，但归档、签名、安装后的最终 release artifact 仍未验证，不能据此声明
产品可发布。

架构保持唯一：`agentd` 持有 Runtime、模型、工具、MCP 与 EventStore，TUI 只是认证
gRPC 展示 client。不得增加 embedded Runtime、GUI fallback 或 Rust 兼容路径。

## 后续顺序

1. 在已完成的 [System One 决策层](../architecture/system-one-decision-layer.md) daemon
   组合、受保护模型存储、领域事件和 Windows 真实 smoke 基础上，增加 TUI 安装/状态投影与
   大样本中英文准确率、校准 benchmark；达到质量门槛前保持 `continue_pi` advisory 策略。
2. 决策并实现 CPU/Vulkan/CUDA 制品变体、归档/checksum/SBOM/签名和干净机器安装器，
   并基于最终 release artifact 在支持的发布硬件上重复固定真实 GGUF 的下载、激活、
   取消、推理、缓存重启与有界关闭验证。
3. 将构建产物认证黑盒从已验证的 spawn 与 setup-required 检查点扩展到复用、重启
   fencing、模型进度、推理与取消。
4. 在已落地的有界 Pi history hydration 基础上实现运行时 context pruning/compaction，
   并用多 turn 真实 GGUF 场景验证 4096-token 窗口。
5. 在现有窄化文件工具与审批基础上增加 MCP 与 Secret；各能力独立验证前继续
   default deny。
6. 对最终制品执行 frozen install、完整 format、lint、typecheck、test、build、docs
   与 `ci-gate`。
