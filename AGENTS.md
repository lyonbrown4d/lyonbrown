# AGENTS.md

本文件统一约定 Lyonbrown 在 Node.js / TypeScript 迁移期间的工程与架构约束。所有贡献者（人类与 AI agent）必须遵守。

## 工具链与代码风格

1. 使用 Node.js 24 LTS 与仓库固定的 pnpm，不混用 npm、Yarn 或 Bun。依赖安装必须更新并提交 `pnpm-lock.yaml`，不得把 pnpm store/cache 重定向到仓库内。
2. TypeScript 使用 ESM 与 `strict` 模式。不得在关键路径使用 `any` 或隐式 `any`；跨包只从公开入口导入。
3. 手写 TypeScript 源文件禁止相对模块导入，统一使用 `@lyonbrown/...` 根路径别名；包内实现使用所属 workspace 的 `/internal/*`、测试 helper 使用 `/test/*`、脚本使用 `/script/*`，跨包仍只能从对方公开入口导入。别名不得携带 `.ts` 扩展名，生产构建必须将别名改写为 Node.js 可执行的 `.js` 路径；协议生成物由生成器管理，不得作为手写代码引用相对路径的先例。
4. 统一使用 2 空格缩进，不使用 Tab。提交前运行根目录的格式、lint、类型、测试和构建门禁。
5. 工具依赖放在 `devDependencies`；版本不得使用 `latest`、`*` 或 `~` 范围。复杂编排脚本放在 `scripts/*.ts`，通过 `tsx` 执行并使用 `zx` 管理子进程。
6. Node 后端的业务级延迟任务、周期任务、重试编排和持久化作业必须优先采用经过维护的第三方库，不得用 `setInterval`、递归 `setTimeout`、自建 cron 解析器或临时数据库表实现调度器。原生 timer 仅用于有界的进程内 deadline、取消、资源清理等待和短退避；需要跨重启、并发 worker、幂等重试或可观测状态时，必须先定义 job port，再选择成熟的持久化队列/工作流引擎。
7. moon 是 monorepo 任务图与缓存的唯一调度器，pnpm 仍是唯一包管理器。根目录的 `dev`、`dev:docs`、`lint`、`typecheck`、`test` 和 `build` 必须经 moon 执行；不得恢复并行的自制 workspace 调度器，也不得让 moon/proto 下载或替换仓库固定的 Node.js 与 pnpm。

## 结构与边界

1. 生产代码与测试代码分离：测试放在独立的 `tests/` 目录或 `*.test.ts` 文件中，不在生产模块中内嵌测试。
2. 手写源文件达到或超过 300 行时，必须评估能否按职责、领域、层或可独立测试行为拆分。仅为压缩行数不得破坏内聚性；保留大文件时需在评审中说明理由。
3. Pi Agent Core、Pi AI 与 Pi TUI 的具体类型只能存在于适配层；不得泄漏到 gRPC/proto 公共契约或 UI view model。
4. `apps/agent-cli`（TUI）只是经过认证的 client，不拥有 Runtime、模型、工具、MCP 连接或 EventStore；这些能力由 `apps/agentd` 组合和持有。
5. EventStore 是会话与执行状态的真相源，Pi 是执行引擎。执行结果必须先映射为 Lyonbrown 领域事件，再投影给 client。
6. 仓库只保留 Node.js / TypeScript 产品实现。不得重新引入 Rust runtime、GUI/Desktop client 或双实现兼容路径；历史行为需要保留时，转换为语言无关 fixture 或 TypeScript 回归测试。
7. 新的工程风格与结构约束必须收敛到本文件，不散落为口头约定。
8. 每个 `apps/*`、`packages/*` workspace 包都必须在 `.moon/workspace.yml` 中注册，并在 `.moon/projects/*/moon.yml` 显式维护内部依赖边、输入与输出。新增、删除或重命名 workspace 包时必须同步更新 moon 项目图和配置契约测试。

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

涉及迁移行为时，必须补充 TypeScript 回归测试、协议 fixture 或黑盒测试；不得依赖已移除的 Rust oracle。
