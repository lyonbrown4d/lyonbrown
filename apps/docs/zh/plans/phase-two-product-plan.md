# 第二阶段产品与交互实施计划

> 状态：Proposed，作为 M0–M9 MVP 之后的实施基线
> 范围：模型控制面、共享应用层、TUI、GUI 与跨端交互契约
> 依赖：[本地优先 Agent Runtime 架构](../architecture/local-first-agent-runtime.md)、[第一阶段 MVP 计划](mvp-implementation-plan.md)

## 1. 目标

第二阶段不再以“补齐更多 Provider 或界面组件”为主，而是把 Lyonbrown 的核心差异转化为可感知、可解释、可恢复的产品体验：

1. 模型是按角色和策略调度的算力，用户可以理解、约束和切换，而不会丢失 Agent 状态。
2. TUI 与 GUI 共享同一套 Runtime 行为、事件投影、审批语义和会话语义，不各自实现一套 Agent。
3. 默认本地运行，但模型下载、资源占用、远程升级和数据边界必须显式可见。
4. 对话是主界面；模型选择、工具执行、审批和恢复以渐进披露的活动时间线呈现，而不是把原始日志倾倒给用户。

第二阶段的完成标准不是“GUI 能显示聊天”，而是以下闭环在 TUI 和 GUI 中语义一致：

```text
选择或自动配置模型
        ↓
创建 / 恢复 Runtime Session
        ↓
提交任务并看到路由原因
        ↓
审阅和决定高风险工具调用
        ↓
取消、失败、回退或继续
        ↓
重启应用后恢复同一工作状态
```

## 2. 当前基线与缺口

### 2.1 已有基础

- Kernel、状态机、AgentEvent、SQLite 恢复和取消语义已经落地。
- 产品已统一本地 C/S 边界：`agentd` 持有 Kernel，TUI 与 GUI 都通过已认证
  gRPC 契约访问。
- llama.cpp 在 `agentd` 中默认启用；GGUF 缺失时进入
  `ModelSetupRequired`，不会在启动时触发下载。
- 默认托管 preset 是 `qwen3-1.7b-q4_k_m`。daemon 已支持显式后台下载、取消和
  单一 `RuntimeSnapshot` 进度流；配置路径缺失时不会伪装成可下载 preset。
- `agentd` 已使用 in-process epoch supervisor 激活安装后的模型；replacement 构建
  和新 endpoint 发布期间保留旧 control plane，失败时回滚到可重试状态。
- TUI 与 GUI 已完成共享模型设置闭环：通过同一 Snapshot/RPC 契约显式下载或重试、
  展示进度、取消、保留失败详情，并支持英文/简体中文呈现。
- TUI 对已识别的简体中文系统 locale tag 选择 `zh-CN`，否则回退到 `en-US`；全局
  `F2` 可从 setup、approval 或 conversation 切换 client 文案，本地 `/language`
  命令不访问 daemon，也不改变 transcript/Session state。
- client command 在入队前绑定已接受的 server epoch；类型化 `EpochMismatch`、
  `StaleEpoch` 映射和严格 receipt 校验共同阻止旧 epoch 命令进入 replacement
  Runtime。
- Ollama、Anthropic 和 OpenAI 仍是可配置 Provider。
- Scheduler 已有 Control / Worker / Frontier、Local / Remote、隐私过滤和有限回退。

### 2.2 主要缺口

- `model.provider + model.model` 仍以单一主模型配置为中心，生产路径没有形成真正的角色化模型池。
- 两端共用的模型设置操作项已实现；更完整的磁盘/资源说明和其余角色化模型管理
  界面仍需补齐。
- TUI 与 GUI 已共享远端投影，但完整产品交互闭环仍需端到端验证。
- TUI 已基于一份固定的十命令目录提供双语命令面板和可发现的帮助；真实的会话/
  模型选择器仍未完成；双语、基于 Snapshot 的只读活动检查器已经落地，但还不是带
  时间戳的完整 event trace。
- 虽然 TUI 已可直接切换语言，但完整 Theme/Language 控制面板和 locale 偏好
  持久化仍属于 M12。
- GUI 已能提交真实远程 `StartTurn` 并订阅 daemon 状态，但会话 transcript、
  流式输出、审批、取消与恢复闭环仍未完成。
- 当前 Quality Gate 只处理模型错误和空答案，不能支撑成熟的质量升级策略。
- Shell 仍是 host/unsandboxed；界面必须如实展示，不能用视觉设计暗示已经隔离。

## 3. 本阶段确认的设计决策

### D10. TUI 是参考客户端，GUI 是同等能力的图形客户端

TUI 先实现和验证新的交互契约，因为它测试成本低、适合 SSH 和键盘工作流。
GUI 不复制 TUI 布局，但两端必须使用相同的 gRPC 命令、投影和审批语义；任何
client 都不能直接访问 Kernel。

“参考客户端”不代表 GUI 是次要产品；它表示任何新的 Runtime 能力先在共享协议和 TUI 验证，再进入 GUI，避免两个客户端同时猜测 daemon 行为。

### D11. 新增 UI 中立的应用层，不让客户端直接编排 Kernel

`crates/agent-app` 保持在 `agentd` 后方，职责限定为：

- 接收类型化 `AgentCommand`，例如创建会话、提交任务、取消、恢复、选择模型和回应审批；
- 持有 Kernel 和会话生命周期，不把可变 Kernel 在 UI 任务之间搬运；
- 将 AgentEvent 投影为稳定的 `RuntimeSnapshot`、`TurnView`、`ActivityView` 和 `ApprovalView`；
- 通过 gRPC service 暴露订阅流和一次性快照；
- 不包含 Ratatui、GPUI、颜色、布局或按键知识。

`agentd` 是唯一 Composition Root。TUI 与 GUI 使用 `agent-client`，不保留直接
或 headless Kernel 兼容入口。

### D12. 模型配置从“单模型”升级为“角色 + 策略”

模型角色固定为：

- `router`：低成本、低延迟的类型化任务分类；不可用时退回确定性规则。
- `worker`：默认执行模型，优先本地，负责工具调用和主要推理。
- `frontier`：按策略启用的远程升级模型，不是默认必经路径。

会话默认使用 `Auto`，允许用户把 `worker` 固定到某个模型；切换只改变后续 step，不创建新会话、不清空工作记忆。第二阶段暂不做每条消息的临时模型覆盖，避免范围和解释成本失控。

Contract 节点已经完成：`RuntimeSnapshot` 现在携带 daemon 已配置 provider route 的
可选有界目录，以及 active/pending 会话 policy；Worker eligibility 由 daemon 计算，
client 不自行推断。当前 `Auto` 的准确含义是“没有会话 override，使用 daemon 基础
配置与路由策略”。provider 探测有界且全有或全无，不阻塞启动，也不发布部分目录；
成功后冻结一份同时供发布与调度使用的能力快照。身份不一致、重复/超长 route 和目录
溢出会使 activation fail closed。selection 尚未实现时，即使基础配置指定了模型，
active 也始终为 `Auto` 且 pending 为空。
这个节点**尚未**实现真实 selection command，也未实现 TUI/GUI 模型 picker；它们仍
属于 M12 后续工作。

角色化 schema 直接替换旧 `[model]` schema，不在 Runtime 中保留兼容映射。
迁移必须显式完成，配置错误按 fail closed 处理。

```toml
[models]
mode = "auto"

[models.router]
provider = "llamacpp"
model = "auto"
load = "lazy"

[models.worker]
provider = "llamacpp"
model = "auto"
load = "resident"

[models.frontier]
provider = "anthropic"
model = "claude-sonnet"
enabled = false

[routing]
prefer_local = true
allow_remote_fallback = false
max_remote_cost_usd = 1.0
```

### D13. 本地优先不等于静默下载

首次需要内嵌模型时，Runtime 必须先返回结构化 `ModelSetupRequired`，由客户端展示：

- 模型、量化、来源和许可证链接；
- 下载大小、最大落盘大小、SHA-256 状态；
- 当前 RAM、可选 GPU backend 和预计运行模式；
- 下载、选择其他模型或退出三个动作。

只有缓存已通过校验时才允许无交互继续。下载必须由 client 显式发出 unary
`StartModelDownload`，取消使用 `CancelModelDownload`；不提供 headless `--yes`
绕过。进度和结果进入唯一的版本化 `RuntimeSnapshot` stream，不创建第二条模型
事件流。

托管状态携带 source、phase、allowed actions、字节进度、failure 和
`operation_id`。关闭 client 不取消 daemon 任务；Cancel 必须匹配当前 operation，
进入 `Installing` 后返回 TooLate。失败后的动作叫 Retry，不叫 resume。client 只
提交当前配置对应的内置 preset ID，URL、摘要和目标路径均由 daemon 目录决定。
显式配置的 `model_path` 缺失时不能发起托管下载。

安装后的激活由 in-process epoch supervisor 完成。旧 control plane 在 replacement
Runtime 构建时继续服务；新 endpoint 启动并原子发布 descriptor 后才停止旧
endpoint。地址、token 与 server UUID 轮换，scope、PID 与生命周期锁不变，client
自动 reconnect/resync。构建或切换失败保留旧 control plane 并 fail closed；
Runtime shutdown 显式执行 graceful cleanup，必要时再 abort 残留任务。

### D14. 对话、活动和检查器是三个不同的信息层

- **对话**只保留用户意图、Agent 回答和需要用户行动的内容。
- **活动时间线**按 step 聚合模型选择、工具调用、回退、错误和恢复事件，默认折叠。
- **检查器**提供原始参数、token、模型能力、上下文裁剪、事件和诊断信息。

客户端不得把每个 AgentEvent 都渲染成一条聊天消息，也不得把关键审批藏在仅供调试的日志里。

### D15. 审批必须由工具提供可验证的预览

在 Tool trait 旁新增 UI 中立的 `ToolPreview`：目标、动作、影响范围、风险、sandbox 状态和所需 SecretRef 名称。预览由工具实现根据类型化参数产生，不依赖模型自由文本自述。

审批动作在 TUI 和 GUI 中保持一致：

- Allow once
- Allow this tool for session
- Deny once
- Deny remaining risky actions

默认焦点为拒绝；窗口关闭、客户端断开或超时均 fail-closed。Shell 必须显示完整命令、工作目录、环境变量名称和 `host/unsandboxed`，但不得显示 Secret 值。

### D16. 保留 GPUI，但在真实垂直切片之后再决定长期承诺

第二阶段继续使用已经 pin 的 GPUI，不立即重写 GUI。原因是当前主要风险是应用层和交互闭环，而不是渲染框架。

完成 M13 后以以下条件进行一次 framework gate：

- Windows、macOS、Linux 的构建与启动可持续维护；
- 中文 IME、复制粘贴、键盘导航、缩放和长对话性能可接受；
- 关键控件具备可测试的焦点与可访问性语义；
- GPUI 升级成本没有迫使业务代码绑定内部 API。

若未通过，只替换 GUI adapter；`agent-app` 和交互契约不变。

## 4. 模型控制面设计

### 4.1 模型目录与生命周期

新增统一模型清单，区分：

- `available`：Provider 能发现但未验证可用；
- `ready`：凭据、endpoint 或本地文件已经验证；
- `downloading`：具有进度、速率、剩余字节和取消句柄；
- `loading` / `resident`：内嵌模型正在加载或驻留；
- `unavailable`：保留结构化失败原因和修复动作。

CLI 提供：

```text
agent models list
agent models status
agent models install <preset>
agent models use <model>
agent models remove <preset>
agent models doctor
```

GUI 的 Model Center 与这些命令调用同一服务，不另外实现下载器。

### 4.2 资源策略

自动选择从“只看启动时可用 RAM”扩展为：

1. 探测 CPU、RAM、VRAM、可用 backend 和磁盘空间；
2. 过滤无法运行的模型；
3. 在用户设定的资源上限内选择最大可用 Worker；
4. Router 默认复用 Worker，只有基准证明收益后才常驻第二个小模型；
5. 模型加载失败时提供降级建议，不静默切换到远程模型。

默认只常驻一个重量级 Worker。Router 使用懒加载或复用 Worker，避免“为了省推理成本却额外占满内存”。模型池后续可以加入 LRU 卸载，但不作为 M11 的前置条件。

### 4.3 路由与解释

每次选择产生结构化 `RoutingDecision`：

```text
role + selected model + capability filters + privacy decision
+ estimated cost + fallback eligibility + human-readable reason
```

默认界面只显示当前模型和发生回退时的提示；检查器可展开完整原因。任何远程升级都必须同时满足：Provider 已启用、策略允许、数据分类允许、预算允许。

### 4.4 模型验收

- 所有路由单测使用 MockProvider，不依赖网络或真实 GGUF。
- 每个 Provider 保留协议级 wire tests。
- llama.cpp 保留小 GGUF smoke test，并增加加载取消、资源不足和损坏缓存场景。
- 建立固定任务集，测量首 token、总延迟、工具调用成功率、格式遵循和内存峰值。
- Quality Gate 的升级必须有可测信号；第二阶段不接受模型对自己回答做无约束自评分。

## 5. 共享交互契约

### 5.1 可见状态

所有客户端从同一投影得到以下状态：

```text
Initializing
ModelSetupRequired
Ready
Preparing
Streaming
AwaitingApproval
RunningTool
Cancelling
Completed
FailedRecoverable
FailedTerminal
```

状态必须来自 AgentEvent 和应用命令结果，客户端不得用计时器猜测 Kernel 是否忙碌。

### 5.2 通用动作

- New session / Resume session
- Submit / Cancel turn / Force stop
- Select Auto or pin Worker model for session
- Approve or deny tool request
- Expand activity / inspect details / copy content
- Retry recoverable operation
- Open settings / help / diagnostics
- 预览并保存主题 / 切换界面语言

切换模型、打开设置和展开详情不能中断当前 Session。正在执行工具或等待审批时，改变模型只对下一个 step 生效。

### 5.3 中断与退出

- 第一次 Ctrl-C：取消当前 turn；等待审批时等价于 Deny once 后取消。
- 第二次 Ctrl-C：强制停止任务，但仍尝试 flush 已持久化事件。
- Esc：关闭 overlay、退出选择或清除焦点，不再直接退出整个 TUI。
- 退出使用 `/quit`、命令面板或明确的 quit shortcut，并在任务运行时二次确认。

## 6. TUI 设计

详细视觉、键盘和本地化规范以 [TUI 视觉与交互设计基线](../design/tui-interaction-design.md) 为准。

### 6.1 信息架构

默认布局在所有宽度下保持单一对话流：

```text
Context line · workspace · mode · model
Conversation + grouped activity
Composer · live steering · queued follow-up
Status line · cancel · details · commands
```

Sessions、Model Picker、Theme、Language、Inspector 和 Help 使用临时 overlay，不形成常驻侧栏。审批保留在对话上下文内；只有极窄终端的详情视图可以临时全屏。30×8 仍需无 panic，并提供可操作的最小界面。

### 6.2 键盘与命令

- Ready 时 Enter 提交；运行时 Enter 立即引导当前 turn，Alt-Enter 排队到下一步。
- 多行输入使用可配置的 Ctrl-J；无法区分 Alt-Enter 的终端使用 `/queue <text>`。
- ↑/↓ 在 composer 空闲时滚动对话；编辑状态优先移动光标或使用 PageUp/PageDown 滚动。
- `/model`、`/theme`、`/language`、`/sessions`、`/new`、`/resume`、`/status`、`/queue`、`/help`、`/quit` 是第一组 slash commands。
- Ctrl-P 打开命令面板；`?` 在 composer 为空时打开帮助。
- 当前可执行的子集是 `/language`、`/queue <text>`、`/help` 和 `/quit`。尚未实现的
  命令会明确显示为禁用；`/queue` 需要输入消息，因此也在命令面板中禁用。未知的
  slash command 会由客户端本地拦截，不会作为普通 prompt 发送。
- 审批显示完整动作文本和快捷键，默认焦点固定为 Deny once。

### 6.3 活动呈现

每个 step 形成一个活动组：模型选择、思考/流式状态、工具调用、结果和回退。默认显示一句摘要，例如：

```text
● Local worker · qwen3-4b · selected for tool-capable coding task
◆ shell · awaiting approval
↗ Fallback blocked · tool output is confidential
```

展开后才显示参数、token、耗时和事件顺序。工具失败作为可恢复活动进入对话上下文，不自动变成全屏错误。

## 7. GUI 设计

### 7.1 首个真实版本的范围

M13 只实现完整垂直切片：

- Runtime 启动和模型 setup；
- Session 列表、新建、恢复和标题；
- 对话流式输出；
- 活动卡片与 Runtime Inspector；
- 工具审批、取消、错误与重试；
- Model Picker 和当前路由状态。

不在 M13 内构建代码编辑器、终端模拟器、Git 客户端或插件市场。文件和 diff 先只读预览，并允许在外部编辑器打开，避免过早变成低完成度 IDE。

### 7.2 布局

- 左栏：Workspace、Sessions、New Session、过滤与恢复状态。
- 中栏：Conversation、按 step 聚合的 Activity Card、固定 Composer。
- 右栏：Runtime、Context、Tools、Diagnostics 四个 inspector tab。
- 小于 900 px：左右栏收进 drawer；审批使用 modal/side sheet，不挤压对话。
- Model Center 和 Settings 是独立页面，不塞入聊天右栏。

### 7.3 交互和可访问性

- 完整键盘导航和可见焦点；焦点顺序与视觉顺序一致。
- 支持中文 IME、Unicode grapheme、选择/复制、Markdown code block 和长输出虚拟化。
- 颜色不作为状态的唯一表达；模型、本地/远程、风险和错误均有文字或图标标签。
- 动画只用于表达状态变化，并遵守 reduced-motion；流式输出不能造成布局持续跳动。
- 审批时保持原任务上下文可见，并默认聚焦安全动作。

## 8. 实施里程碑

### M10 — 共享应用层与交互契约

交付：

- `agent-app` 的 Command、Snapshot、Activity、Approval 类型；
- 从 AgentEvent 到视图状态的纯 reducer；
- Kernel 驱动任务与取消/审批 channel；
- TUI 迁移到共享应用层，行为保持不退化；
- GUI 删除静态 sample state，先接入 mock application service。

验收：相同 scripted event trace 在 TUI/GUI 投影出相同状态；resume、cancel、approval 和 lagged-event 有集成测试。

### M11 — 模型控制面

状态：daemon 下载/取消、Snapshot 进度、完整性校验、epoch 激活，以及 TUI/GUI
共用模型设置操作项均已落地；两端在该闭环中支持英文和简体中文。命令被 fence
到已接受的 server epoch，且只有 receipt 校验通过才报告成功。这里完成的是共享
setup slice，不代表 M11 的所有交付项均已完成。

交付：补齐角色化配置、模型目录、doctor/install/status、更完整的硬件/资源说明、
session model pin、结构化 RoutingDecision 和旧配置迁移。TUI 与 GUI 必须继续只
使用相同的 Snapshot/RPC，不实现第二套下载器。

验收：首次运行不会静默下载；离线已有缓存可启动；资源不足和远程回退被界面准确解释。

### M12 — TUI 产品化

命令面板/帮助切片已经落地：同一份双语十命令目录驱动 slash 解析、`Ctrl-P`、
composer 为空时的 `?` 和 `/help`，并覆盖 30×8、52×12、90×20 三档布局。模型设置
与审批的安全交互优先于浮层；审批时第一次 `Ctrl-C` 会尝试 Deny once 并取消，
若审批仍在继续，第二次会强制停止并退出。

活动检查器 MVP 也已落地。`Ctrl-O` 打开已接受 Snapshot 活动的只读投影，提供稳定
turn/step 选择、跟随最新/pin 目标、明确的 fresh/stale 缓存状态，以及 Session/
daemon epoch 清理。中英文界面在 90×20 使用左右双栏、52×12 使用上下堆叠、30×8
使用单窗格。现有详情包括路由/fallback/上下文/用量证据、工具参数/结果/耗时、
outcome 和相关 ID。revision/epoch 门控阻止被拒绝的远端状态局部更新界面；缓存以及
字段、JSON、集合、行数和字符总量预算确保渲染工作与输出有界。

这还不是完整 event trace：时间戳、严格事件历史、历史 `ToolPreview`/capabilities 和
完整 diagnostics 尚未提供，剪贴板复制也未实现。下一小节点是真实模型/会话选择器；
完整里程碑仍包含主题选择浮层及偏好持久化。M12 整体尚未完成。

交付：单一对话流、overlay/command palette、session/model/theme/language picker、六套内置主题、英中本地化、活动组、实时引导与排队、内联审批、帮助、中断新语义、窄屏适配和可复制详情。

验收：30×8、52×12、90+ 宽度均有中英文渲染测试；六套主题覆盖关键状态；Unicode/IME 可表达输入、引导、排队、滚动跟随、审批和取消均有交互测试。

### M13 — GUI Runtime 垂直切片

模型设置卡片、Runtime phase/progress 投影和顶部栏中英文切换已经落地。下一小
节点是完整 GUI transcript、流式输出、审批、取消与恢复闭环，以及响应式三栏
Runtime 界面。

验收：使用 MockProvider 完成端到端桌面测试；Windows 手工 smoke；没有“preview only”或虚假运行状态。

### M14 — GUI 管理与工作流

交付：Model Center、Settings、诊断导出、文件/差异只读预览、外部编辑器跳转、通知和恢复入口。

验收：本地模型从未安装到可用的完整流程无需命令行；失败可重试、取消并给出修复动作。

### M15 — 高级路由与可靠性

交付：基准驱动的独立 Router 决策、改进 Quality Gate、模型加载/卸载策略、成本与延迟预算、路由历史和恢复策略。

验收：固定任务集证明引入第二模型带来可测收益；隐私、预算和工具观察阻止自动远程升级的回归测试保持通过。

### 并行安全轨 — Sandbox 与审计

跨平台 Sandbox 继续按既有设计推进。任何阶段如果仍使用 host shell，TUI/GUI 都必须展示真实状态。Sandbox backend 接入后只改变 `ToolPreview.sandbox` 和执行实现，不改变审批动作或客户端流程。

## 9. 质量门槛

每个里程碑必须满足：

```text
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets
pnpm --filter @lyonbrown/docs build
```

另外要求：

- 单文件不超过 300 行，生产代码与测试代码分离；
- UI reducer、路由和下载状态机使用确定性测试；
- 真实 Provider、GGUF 和桌面 smoke tests 与默认离线测试分层；
- 所有用户可见失败都有恢复动作，不只显示底层错误字符串；
- TUI 和 GUI 的能力可以不同步上线，但同名动作的语义不得不同。

## 10. 决策门与暂缓项

以下事项不在现在凭偏好决定，必须由前一阶段证据触发：

| 决策门 | 触发点 | 需要的证据 |
|---|---|---|
| 单一产品包还是 TUI/GUI 独立 bundle | 首个公开安装器之前 | 两种方案都必须原子携带完全同版本的 sibling `agentd`，比较安装体积、升级行为与发布复杂度 |
| 是否常驻独立 Router 模型 | M15 | 延迟、内存和任务成功率相对复用 Worker 有明确收益 |
| 是否继续长期使用 GPUI | M13 完成后 | 三平台、IME、可访问性、性能和升级成本 |
| 是否加入每 turn 模型覆盖 | M14 后 | 用户确有频繁临时切换需求，且路由解释仍清晰 |
| 是否引入多 Kernel 并发 Session | M13 后 | GUI/TUI 的后台任务和资源模型已经定义 |
| 是否实现内置代码编辑器 | M14 后 | 只读预览与外部编辑器流程无法满足核心任务 |

第二阶段明确暂缓：Sub-agent 编排、长期向量记忆、插件市场、远程 Sync Server 和完整 IDE。它们不应阻塞模型控制面和两个客户端的基本闭环。
