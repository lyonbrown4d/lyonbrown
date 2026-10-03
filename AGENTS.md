# AGENTS.md

本文件统一约定 Lyonbrown 在 Node.js / TypeScript 迁移期间的工程与架构约束。所有贡献者（人类与 AI agent）必须遵守。

## 工具链与代码风格

1. 使用 Node.js 24 LTS 与仓库固定的 pnpm，不混用 npm、Yarn 或 Bun。依赖安装必须更新并提交 `pnpm-lock.yaml`，不得把 pnpm store/cache 重定向到仓库内。
2. TypeScript 使用 ESM 与 `strict` 模式。不得在关键路径使用 `any` 或隐式 `any`；跨包只从公开入口导入。
3. 统一使用 2 空格缩进，不使用 Tab。提交前运行根目录的格式、lint、类型、测试和构建门禁。
4. 工具依赖放在 `devDependencies`；版本不得使用 `latest`、`*` 或 `~` 范围。复杂编排脚本放在 `scripts/*.ts`，通过 `tsx` 执行并使用 `zx` 管理子进程。

## 结构与边界

1. 生产代码与测试代码分离：测试放在独立的 `tests/` 目录或 `*.test.ts` 文件中，不在生产模块中内嵌测试。
2. 手写源文件达到或超过 300 行时，必须评估能否按职责、领域、层或可独立测试行为拆分。仅为压缩行数不得破坏内聚性；保留大文件时需在评审中说明理由。
3. Pi Agent Core、Pi AI 与 Pi TUI 的具体类型只能存在于适配层；不得泄漏到 gRPC/proto 公共契约或 UI view model。
4. `apps/agent`（TUI）和 `apps/desktop`（GUI）只是经过认证的 client，不拥有 Runtime、模型、工具、MCP 连接或 EventStore；这些能力由 `apps/agentd` 组合和持有。
5. EventStore 是会话与执行状态的真相源，Pi 是执行引擎。执行结果必须先映射为 Lyonbrown 领域事件，再投影给 client。
6. 迁移期间 Rust 实现是行为 oracle。完成对等测试与切换验收前，不得随意删除 Rust crate、provider、测试或 Cargo 基线，也不得用未验证的 TypeScript 行为覆盖它们。
7. 新的工程风格与结构约束必须收敛到本文件，不散落为口头约定。

## 验证

提交前至少执行：

```text
pnpm install --frozen-lockfile
pnpm preflight
pnpm format:check
pnpm lint
pnpm typecheck
pnpm test
pnpm build
pnpm ci-gate
```

涉及迁移行为时，还必须运行对应的 Rust oracle/parity 测试；通过 parity 前不得声称迁移完成。
