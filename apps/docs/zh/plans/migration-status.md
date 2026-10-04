# 迁移暂停检查点

> 暂停日期：2026-10-04
>
> 本页记录工程检查点，不表示已经可发布。迁移工作现已暂停；没有新的明确继续
> 决策时，不应恢复实现任务。

## 已合入 `migration/node-typescript-pi`

- Node.js 24、pnpm、strict ESM、仓库门禁与双语 VitePress。
- TypeScript domain、protocol/gRPC、认证 client、launch coordinator、durable
  Runtime 适配器、Pi kernel 适配器、SQLite EventStore、本地 control plane、
  Windows 原生路径校验和本地模型基础。
- GUI/Desktop 产品代码与计划已经删除。
- `2594ad9` 删除了根 Cargo 元数据、Rust crates/providers、`agentd` 的 Rust
  半边和 Rust 构建脚本。
- Node protobuf 源成为唯一 schema 真相源；版本化 SHA-256 fixture 与反射断言
  接替已删除的 Rust parity 测试。
- workspace identity golden vectors 改用语言无关名称，原始 bytes 与预期 scope ID
  保持不变。

清理批次已通过规格审查；其代码质量审查在收到暂停指令时被中断，因此仍未完成。

## 已保存但未合入

### Node TUI 检查点

- 分支：`feature/agent-cli-node`
- Commit：`b71a6b6`
- 在原路径将 `apps/agent-cli` 替换为 TypeScript，并精确固定
  `@earendil-works/pi-tui@1.0.0`。
- 已实现经过测试的 vertical slice：默认直接 chat、立即显示 connecting、认证
  launcher 注入、Snapshot streaming 与 revision fencing、中英文、六套主题、模型
  设置、审批、命令面板、Activity Inspector 和终端文本安全。
- 暂停前重新验证：lint 与 typecheck 通过，18/18 tests 通过。
- package build 尚未通过：重复构建依赖时，`@lyonbrown/agent-launch` 无法解析
  `@lyonbrown/local-control-plane`。
- 生产可执行文件仍需要具体的安全 launcher；未注入时会按设计返回
  `SECURE_AGENT_LAUNCHER_REQUIRED`。
- 尚未完成独立规格审查与代码质量审查。

在该分支修复并合入前，迁移分支仍暂留旧 `apps/agent-cli` Rust 源，作为不参与
workspace 的静态快照。根 Cargo 元数据已经删除，它不是受支持的构建路径。

### Windows 原生模型存储检查点

- 分支：`feature/native-model-storage`
- Commit：`e389879`
- 包含 Windows DACL、reparse point、文件身份、install lock、staging、size bound、
  原子激活和 installed-file anchoring。
- 该 package 曾报告 84 项测试通过，statement/branch 覆盖率超过包门禁。
- 该提交存在 blocker，禁止合入：真实 `node-llama-cpp` downloader 先写
  `filename.ipull`，再 rename 到最终文件；当前 staging 提前以不共享 delete 的
  句柄打开最终文件，导致 rename 返回 `EPERM`，失败清理还可能遗留 staging 目录。

## 当前产品状态

迁移分支目前不是可端到端运行的产品检查点。真实生产 `agent` 仍缺少：

- 经过审查并合入的 Node TUI；
- 具体的原生发现、进程启动和认证 client adapter；
- `agentd` 生产组合根与有界关闭装配；
- 修正后的原生模型存储/下载契约；
- 完整工具、MCP、审批、上下文、Secret、打包与发布验证。

不得为了绕过这些缺口重新引入 embedded Runtime、GUI fallback 或 Rust 兼容 runtime。

## 恢复顺序

1. 使用真实 `.ipull` rename 集成测试和清理验证修复并审查
   `feature/native-model-storage`。
2. 修复 TUI 依赖构建失败，加入具体 launcher composition，再完成
   `feature/agent-cli-node` 的规格与代码质量审查。
3. 合入 Node TUI，同时删除 `apps/agent-cli` 下最后的静态 Rust 源。
4. 完成 `agentd` 生产组合，并运行认证本地 daemon/TUI 端到端测试。
5. 在声明可运行或可发布前，执行 frozen install、完整 format、lint、typecheck、
   test、build、docs 与 `ci-gate`。
