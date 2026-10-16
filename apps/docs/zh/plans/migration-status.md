# 迁移进度

> 已验证检查点：2026-10-14
>
> 后续开发直接在 `main` 上继续。本页会区分“已经实现并有测试覆盖”和“发布验证”，
> 不表示产品已经可以发布。

> 2026-10-16 路线调整：近期先将操作能力升级为 Windows、macOS、Linux 跨平台实现，
> 并完成单机真实任务验收，再推进本地 Root/Subagent 和分布式执行。
> 规划与当前工作区实现分别记录；历史验收结果不代表本轮三平台均已验证。

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
- 旧 GUI/Desktop、根 Cargo 元数据、Rust crates/providers、Rust 构建脚本和最后的
  Rust TUI 快照均已删除。受跟踪仓库中不再包含产品 `.rs`、`Cargo.toml` 或
  `Cargo.lock` 文件。
- 新的 Electron Desktop 客户端骨架已经合入，复用 launcher、认证 gRPC 和 daemon；
  当前交互和待补能力见 [Electron Desktop](../architecture/electron-desktop.md)。
- Windows 原生模型存储已经覆盖私有 ACL、reparse point 与文件身份校验、安装锁、
  有界 staging、原子激活和 installed-file anchoring；成功激活还会安全清理同模型的
  崩溃残留 staging。
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

- 本轮工作区已完成决策模型 TUI 安装与状态投影，独立安装子进程、模型用途隔离、
  operation/epoch fencing、缺失时继续 chat、成功安装后的懒加载切换均有回归测试。
  这些改动纳入本轮 `main` 检查点。
- 2026-10-14 真实独立 daemon/gRPC/ConPTY 验收验证了设置页打开决策面板、英文状态、
  F2 中文切换和退出码 `0`。36 条双语探索性模型基准已完成，意图/风险/工具需求准确率
  分别为 55.6%/38.9%/75.0%，继续固定采用 `continue_pi`。

- 包级回归测试覆盖 TUI、Windows 原生模型存储、protocol 冷生成，并会拒绝受 Git
  跟踪的 protocol 生成输出。
- 下载回归测试覆盖内建 `fetch` 的流式写入、响应 headers 前有限重试、取消传播、长度
  与字节上限校验，以及失败后的 staging 清理。
- 聚焦的 TypeScript 测试已经覆盖生产组合、model-aware Runtime 激活与重试、
  transport revision、本地推理并发与清理、Session 轮换和重启恢复。
- 当前本轮聚焦测试数量为：`agentd` 267、`local-model` 136、脚本编排 60；完整仓库门禁
  仍以本页末尾列出的命令为准。
- 构建后的生产 launcher/独立 `agentd` 启动黑盒已于 2026-10-06 通过：
  `source=spawned`、认证 health 为 `serving`、模型设置为 `required`、
  `allowedActions=[download]`、投影 revision 为 `2`。全程没有自动生成 GGUF，结束后
  临时 daemon 与目录均已清理。
- Windows 开发机上的独立 `agentd` 真实 GGUF smoke 已于 2026-10-08 通过。脚本从
  可信目录读取实际硬件选择及固定摘要，验证了不自动下载、显式取消/重试、进度、校验、
  激活、pinned/auto 选择、真实回答和 usage、`write` 调用、审批与准确落盘、长 turn 取消、
  新 PID/server epoch 的缓存重启以及 30 秒预算内关闭。模型持久保存在 dotenv 注入的
  `F:\Lyonbrown\models\chat`，脚本最终输出 `REAL_GGUF_SMOKE_OK`，并只清理单次 scratch。
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
尚未接入。Electron Desktop 已有客户端骨架，完整产品交互和安装分发尚未完成；移动端仍不在当前范围。
真实 GGUF 下载、激活、推理、工具、取消、
缓存重启与有界关闭已经在当前 Windows 开发机的生产 composition 上通过；可移动开发
目录已经验证，但归档、签名、安装后的最终 release artifact 仍未验证，不能据此声明
产品可发布。

架构保持唯一：`agentd` 持有 Runtime、模型、工具、MCP 与 EventStore，TUI 和 Desktop
只是认证 client。不得增加 embedded Runtime、旧 GUI fallback 或 Rust 兼容路径。

## 单机可用性缺口（2026-10-16 代码核对）

近期工程优先项是按操作能力推进跨平台升级，覆盖 Windows、macOS、Linux。每项能力先明确共同契约，再复用通用实现并补齐必要的平台适配，分别验证三平台行为。工程约束维护在根目录 `AGENTS.md`。现有 Windows 实现与验证记录作为迁移基线，本轮已补齐 POSIX 基础适配，并完成 Mac 独立 daemon 启动验证；三平台完整任务验收仍待完成。

近期按以下标准判断单机 Coding 是否可用：从 TUI 或 Desktop 提交真实仓库任务，Agent
能定位代码、修改文件、运行构建/测试、根据失败继续修复并说明验证结果；多轮交互、取消、
重连和重启后的状态必须可理解。聊天和一次文件写入 smoke 不能单独证明达到该标准。

| 优先级             | 当前依据与缺口                                                                                                                                                                              | 下一项验收                                                                                                   |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| P0：操作能力跨平台 | 当前工作区已实现 POSIX 原生目录、锁、GGUF 与决策模型存储，保留 Windows 适配；Mac 独立 daemon/gRPC 验收已通过。CI 已扩展三平台，尚无本轮 Windows/Linux 运行结果。                            | 完成 Windows、macOS、Linux 完整门禁和真实任务验收，分别验证原生依赖、模型加载、工具、取消与重启。            |
| P0：完成代码任务   | registry 已新增 `list_directory`、`search_files` 和高风险审批的 `process_execute`；后者可显式执行 Git 或项目工具。已有真实 argv、搜索边界和子进程取消测试，尚未完成模型驱动的仓库任务验收。 | 在三平台验收“定位 → 修改 → 运行检查 → 修复失败 → 报告结果”。                                                 |
| P0：执行模型质量   | 生产组合使用本地模型；已有真实 GGUF 证据主要覆盖回答、指定文件写入和生命周期，没有真实仓库任务质量基准。                                                                                    | 用明确任务和验收条件评价模型的理解、工具选择、修复成功率与耗时，再决定本地模型或其他 Provider 路径。         |
| P0：任务上下文     | 本地推理固定 4096-token context；Session 有条数/字节上限与历史恢复，尚无应用层运行时 pruning/compaction。                                                                                   | 真实多轮代码任务中，工具输出和历史增长后仍保留目标、修改及验证信息，超限时给出明确可恢复状态。               |
| P1：配置与恢复体验 | CLI 仍传入固定 `production-default` fingerprint；完整有效配置的规范化/指纹尚未接通。恢复仅续接已完成且无歧义的 Session；运行中、取消和待审批等状态会轮换。                                  | 配置变化正确隔离 daemon scope；用户能找到原会话、理解中断结果并继续工作，未决副作用不自动重放。              |
| P1：真实客户端验收 | TUI 有真实 setup/决策面板证据，Desktop 有连接和聊天骨架；真实代码任务的完整可视交互尚缺验收记录。                                                                                           | 在选定客户端完成代码任务、审批拒绝/允许、取消、断线重连与历史查看。                                          |
| 发布前：交付       | Windows 可移动开发目录已验证；最终归档、签名、安装、升级/回滚和干净机器验证仍未完成，Desktop 分发也待实现。                                                                                 | 为三平台维护独立制品与验收记录；基于各平台最终制品复验安装、真实任务、缓存复用和清理，再声明对应平台可发布。 |

### 跨平台实现检查点（2026-10-16）

- POSIX 使用原生 `openat`、`O_NOFOLLOW`、`flock`、目录句柄及身份复核；Darwin 还检查扩展 ACL。实现覆盖 macOS/Linux x64 与 arm64，其他平台或架构明确拒绝。
- GGUF 和决策模型 bundle 使用私有目录、完整身份键、受控 staging、原子发布及回调前后校验。POSIX advisory lock 约束本适配器的并发操作，不能阻止同一 OS 用户下的恶意进程写入；Windows 原生 share-deny 保持不变。模型 bundle 的历史版本清理仍待补充。
- 文件工具新增有界目录枚举与字面量文本搜索。进程工具使用参数数组、受限继承环境、有界输出和超时/取消清理，必须经高风险审批；它以主机用户权限运行，不是 OS 沙箱。
- 新增 `pnpm smoke:platform`，验证独立 daemon、原生发现、认证 gRPC、SQLite 和无模型下载启动，成功标记为 `PLATFORM_SMOKE_OK <os>-<arch>`。本机已返回 `darwin-arm64`。
- macOS arm64 已通过 frozen install、preflight、format、lint、typecheck、test、build 和 `ci-gate`。本轮完整门禁包含原生适配 46 项、agentd 277 项、脚本 79 项通过；平台专属跳过项不计入通过数。Windows/Linux 尚无本轮真实运行结果。
- macOS arm64 开发制品通过独立导入、SQLite、平台预编译包与目录搬移 smoke，含空格/中文路径的启动器也已验证。制品与源码硬链接已解除；最终安装分发和真实 GGUF 任务仍待验收。
- 打包与启动器已区分三平台，GGUF/TUI 验收入口不再只允许 Windows，决策模型脚本不再强制 F 盘。CI 已配置三平台质量门禁和独立 daemon smoke；配置存在不等于远端执行通过。

### 跨平台操作升级范围

以下为实施与验收范围，不表示已经完成适配。统一的是操作契约和可观察行为；权限、文件锁、进程终止、终端和签名仍使用各系统适用的机制。

| 能力              | 需要统一的行为                                                   | 平台适配与验收重点                                                                            |
| ----------------- | ---------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| 文件与工作区      | 读写、编辑、目录枚举、搜索、原子替换和工作区访问边界             | 路径分隔符、盘符、大小写、Unicode、符号链接/reparse point、文件身份与访问权限。               |
| 进程与命令        | 参数数组、工作目录、环境、输出、退出结果、取消、超时和子进程清理 | 可执行文件发现、shell 选择与引用规则、进程树终止；避免依赖某个系统命令或 shell 语法。         |
| daemon 与本地安全 | 工作区身份、私有目录、发现、认证、互斥锁和启动/关闭              | Windows ACL 与 POSIX owner/mode、锁与稳定目录句柄提供等价的安全约束。                         |
| 模型与持久存储    | 下载、校验、安装锁、原子激活、缓存、取消和恢复                   | 原生依赖的 OS/架构预编译支持、文件锚定、崩溃清理；分别记录实际验证过的硬件后端。              |
| Git 与客户端      | Git 工作区生命周期、TUI 输入/输出、Desktop 连接与受限 preload    | Git 可执行文件和路径处理、不同终端/PTY 行为、客户端生命周期；桌面交互权限单独声明。           |
| 验证与分发        | 共用能力契约测试、真实任务场景和检查结果格式                     | 为三平台配置运行环境与真实 smoke，逐步替换 Windows 专用验收入口，分别验证制品与安装生命周期。 |

按能力推进顺序为：文件/进程/本地安全 → 模型与存储 → Git 与客户端 → 真实任务与制品。不支持的能力明确返回不可用及原因；原生安全能力缺失时继续拒绝操作。平台支持状态必须标明 OS、架构和运行证据，不能由一个平台的测试推定其他平台可用。

MCP、Secret 和 Skills 的接入按真实任务需要推进；各能力验证前继续 default deny。
[System One 决策层](../architecture/system-one-decision-layer.md) 保持 `continue_pi`，扩大
独立评测集和校准工作可继续，但分类质量暂不作为普通任务执行的前置门槛。

## 后续迭代主线：跨平台操作 → 单机可用 → 分布式 Agent

2026-10-16 已确认将分布式架构纳入后续主线。执行顺序如下，各阶段以验收结果推进，
不把方案中的候选模型、传输库或自动恢复能力视为已经确定或实现。

1. **操作能力跨平台与单机真实任务可用。** 按能力完成 Windows、macOS、Linux 的契约、适配和验收，解决上述 P0，再分别完成客户端、模型、工具、上下文和事件恢复的真实任务验证；发布能力按平台单独验收。
2. **任务契约与本地多 Agent。** 区分 Node、Agent、Run、Task 和 Attempt，定义依赖、
   授权、预算、结果提交与 Root 验收。Root 在本机创建两个独立 Subagent，隔离会话和
   工作区，并由可替换的本地调度接口执行。Pi 继续留在适配层。
3. **跨节点 Subagent。** 增加持久设备身份、加入与执行授权、安全传输、节点能力、
   原子接单、租约、取消和断线结果回传，再接入 LAN/VPN 候选发现。单机运行不依赖中心服务。
4. **分布式 Coding，再验证数据采集。** Coding 使用固定基线和独立 Git 工作区，回传
   commit、日志与测试证据，由 Root 验收和集成；数据采集复用任务协议验证结构化产物。
5. **状态复制、主动交接与可选 Center。** 保持 Root Session 单写入权，增加增量同步、
   `ownerEpoch` 和明确的协调权交接；没有相应协议和故障测试前，不承诺自动接管。

每个交付检查点执行仓库规定的 frozen install、preflight、format、lint、typecheck、
test、build 和 `ci-gate`，并按变化补充真实场景证据。当前客户端 gRPC 与本地 daemon epoch
继续用于本地边界，不能直接当作节点间协议或 Root 协调权实现。
