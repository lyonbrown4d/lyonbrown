# 迁移进度

> 恢复日期：2026-10-05
>
> 后续开发直接在 `main` 上继续。本页记录已经验证的工程状态，不表示产品已经
> 可以发布。

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

## 验证证据

- `@lyonbrown/agent-cli`：lint、typecheck、build 通过，18/18 tests 通过。
- `@lyonbrown/control-plane-native`：lint、typecheck、build 通过，86/86 tests
  通过；statement coverage 90.65%，branch coverage 85.28%。
- ipull 回归测试在真实 Windows 文件系统上执行 rename，然后再封存下载文件；另一个
  失败测试验证遗留 `.ipull` 与 staging 目录都会被删除。
- `@lyonbrown/protocol`：在不存在预生成源码时可完成冷启动 build；lint、typecheck、
  build 与 29/29 tests 通过。仓库策略测试会拒绝受 Git 跟踪或未被忽略的生成输出。
- 完成下一段 composition 工作后、声明可运行或可发布前，仍必须执行仓库级完整
  `ci-gate`。

## 当前产品状态

仓库已经只保留 Node.js / TypeScript 实现，但目前仍不是端到端可运行的产品检查点。
生产路径还缺少：

- TUI launcher 的具体原生发现、进程启动和认证 client factory 适配器；
- 完整的 `agentd` 生产组合根、descriptor 发布、按所有权逆序的有界关闭和可执行入口；
- 认证本地 daemon/TUI 端到端测试；
- 完整工具、MCP、审批执行、上下文、Secret、配置、打包与发布验证。

架构保持唯一：`agentd` 持有 Runtime、模型、工具、MCP 与 EventStore，TUI 只是认证
gRPC 展示 client。不得增加 embedded Runtime、GUI fallback 或 Rust 兼容路径。

## 后续顺序

1. 在 `agent-launch` composition 边界后实现生产 discovery、client factory 与
   process spawner 适配器。
2. 完成 `agentd` 启动/发布和有界的逆序关闭。
3. 运行认证本地 daemon/TUI 黑盒流程，覆盖复用、启动、重启 fencing 与取消。
4. 继续工具、MCP、审批、上下文、Secret、配置、打包与发布切片。
5. 在声明可运行或可发布前执行 frozen install、完整 format、lint、typecheck、
   test、build、docs 与 `ci-gate`。
