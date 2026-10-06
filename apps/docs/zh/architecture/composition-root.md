# `agentd` 组合根

`apps/agentd` 是唯一生产组合根。依赖注入只存在于进程边界，不让领域对象从容器
主动解析服务。生产 launcher 与可执行入口已经使用该组合；不存在第二套进程内或
兼容架构。

> 已验证实现检查点：2026-10-07。构建后的 launcher、独立 daemon 启动与生产
> composition 真实 GGUF 黑盒已通过；发布打包仍待最终制品复验。

## 所有权

daemon 构造并持有：

- 带作用域的 run/data 目录及原生安全能力；
- 生命周期锁、发现描述符、endpoint 与认证 token；
- EventStore、durable Runtime、Pi kernel session 与能力策略；
- 托管 `node-llama-cpp` 模型存储和生命周期；
- 认证 gRPC 服务与 Snapshot stream。

TUI 只能得到窄化后的 `agent-launch`、`agent-client` 与 `ui-model` 接口，不能得到
数据库句柄、模型路径、原始 bearer token、供应商 Secret 或 Pi 对象。Snapshot
顺序使用 daemon 自有的单调 transport revision，不直接暴露 EventStore revision；
仅模型状态与激活状态的更新也进入同一顺序。

当前生产 Kernel 组合受工作区约束的 `read`、`write`、`edit` registry 和显式审批门；
文件能力由 native-required 的 `@openclaw/fs-safe` 根对象提供，读取限制为 1 MiB，
写入使用受约束的原子替换；一次性/会话级审批的完整决策进入领域事件。Secret resolver
仍为拒绝式实现，MCP 尚未连接。工具与审批归 daemon 所有，TUI 只投影状态并提交用户决定。

## Workspace 导入边界

手写 TypeScript 不使用相对模块引用，统一从 `@lyonbrown/...` 根别名导入。每个
workspace 的 `/internal/*`、`/test/*` 与 `/script/*` 都是所有者私有命名空间，仓库
门禁会拒绝其他 workspace 引用；跨包只能经过 `package.json` 声明的公开入口。

Vitest 使用 `vite-tsconfig-paths` 解析源码别名，生产 TypeScript 构建完成后使用
`tsc-alias` 将私有别名改写为带 `.js` 扩展名的 Node ESM 路径。构建产物不得残留私有
别名。被 Git 忽略的 protocol 生成物仍由 Buf 插件生成和管理，不能作为手写代码绕过
导入边界的理由。

## 后台任务与调度

`agentd` 不自建业务调度器。当前仓库中的原生 timer 只承担 RPC deadline、关闭等待、
取消传播和有限退避，不提供跨重启的作业语义。后续出现定时或后台业务任务时按语义
选型：单进程 cron 优先评估 [Croner](https://github.com/Hexagon/croner)；有限 async
重试优先评估 [`p-retry`](https://github.com/sindresorhus/p-retry)；需要持久化、多
worker、重试状态和运维可见性时，通过窄化 job port 接入
[BullMQ](https://docs.bullmq.io/) 或
[Temporal](https://docs.temporal.io/develop/typescript)，不能把 `setInterval` 或
EventStore 临时表扩展成队列。

具体依赖只在首个真实用例出现时引入并精确锁定。选择 BullMQ/Temporal 也意味着接受
对应的 Redis/PostgreSQL 或 Temporal Service 运维边界；本地开箱即用 daemon 不会仅为
“可能以后需要”而强制附带这些服务。所有 job handler 必须幂等、可取消、有执行预算，
并把领域结果先写入 EventStore，再投影给 client。

## 启动顺序

1. 解析并校验类型化进程配置。
2. 使用稳定的文件系统身份与有效配置指纹推导 workspace scope。
3. 打开受保护的 run/data 目录并取得 daemon 生命周期 lease。
4. 打开并恢复 EventStore，再构造本地模型服务。缓存检查在后台开始，且绝不会自动
   启动下载。
5. 在私有 endpoint 启动认证 gRPC server。
6. 最后发布发现描述符；发布时服务必须已可用。
7. 已验证模型就绪后，激活 inference lease 并构造 Runtime 与 Pi 适配器。激活失败
   会保持可观察，用户可以显式重试，且无需重新下载模型。

描述符发布前发生的失败不会被 client 发现，并按所有权逆序清理。进程绝不回退到
embedded Runtime。

## Runtime 与恢复边界

本地模型适配器以单活跃 inference、固定 4096-token context 向 Pi 流式输出文本。
每条 user、follow-up 或 steering 消息在 daemon 命令入口限制为 64 KiB，与
local-model 的单消息上限一致。Kernel 的 256 KiB 限制是完整构造后 inference 输入的
纵深防御上限，并非单条 turn 消息上限。非活跃 Session 会在达到 32 个历史 turn 或
4 MiB 用户文本后，于接收下一次输入前轮换。turn 活跃期间最多排队 8 条、合计
512 KiB 的 follow-up。预算校验与命令持久化共用 daemon 的一条 FIFO 通道，防止并发
RPC 基于过期状态通过检查。

EventStore 仍是 Session 与执行事件的持久真相源。Pi history hydration 尚未实现，
因此 daemon 重启后会安全轮换任何包含历史 turn 的 Session——包括已完成 Session——
而不是在模型上下文不完整时静默续接。旧 Session 仍可从 EventStore 查询。

## 关闭顺序

daemon 先停止接受 RPC 与 stream，在期限内优雅关闭 gRPC，超时后强制关闭；随后
dispose Runtime 和模型资源、关闭 EventStore、释放 lease 与目录锚点，最后移除发现
状态。每一步都必须尝试；诊断只返回脱敏后的第一个失败，不泄漏凭据或本地敏感路径。

## 测试接缝

生产组合显式注入原生能力；平台实现不可用时 fail closed。测试在同一边界使用 port
与 fake，生产默认值不得选择测试 helper。聚焦测试已覆盖组合、激活重试、revision、
轮换与恢复。构建后的生产 launcher 已拉起独立认证 daemon，观察到 `serving`、
`setup-required`、`allowedActions=[download]` 与 revision `2`，没有生成 GGUF，并清理了
临时进程和目录。该证据只验证启动组合；真实 GGUF 下载、激活、推理、取消与发布打包
仍未验证。
