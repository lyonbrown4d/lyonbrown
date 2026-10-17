---
title: 系统架构总览
---

Lyonbrown 由认证客户端、节点服务 `agentd` 和任务编排层组成。客户端负责交互，`agentd` 管理本节点的执行与持久化，Root 负责拆分任务、管理依赖并验收结果。任务可在本节点或其他 Node 上执行；单机运行与节点直连均不依赖 Center。

本页说明各模块的职责、数据流和写入权限。产品主线采用 Node.js 24 与启用 `strict` 模式的 TypeScript。

启动、认证与客户端边界见[本地 gRPC C/S](/zh/architecture/grpc-client-server/)、[组合根](/zh/architecture/composition-root/)和[Electron Desktop](/zh/architecture/electron-desktop/)。

## 总体关系

下图按本地执行、任务编排和执行节点划分职责。Root 是逻辑角色，可将任务派给本节点或其他节点；分组不规定 package、微服务或部署目录。虚线仅表示可选 Center 的连接。

```mermaid
flowchart TB
  User["用户目标与交互"]

  subgraph Local["本地节点 · 执行与持久化"]
    Client["TUI / Electron Desktop<br/>Desktop: renderer → 受限 preload → main<br/>独立 Node 24 client worker"]
    Access["agent-launch / agent-client / protocol<br/>发现、必要时拉起、认证 gRPC<br/>命令与 Snapshot"]
    Boundary["agentd · 唯一组合根<br/>鉴权 / 命令 FIFO / 预算与代次校验"]
    Runtime["agent-runtime / agent-domain<br/>领域事件、执行与安全恢复"]
    Decision["DecisionProvider / daemon 策略<br/>前置决策或安全回退<br/>确定性策略约束动作"]
    Pi["kernel-pi<br/>Pi 执行引擎适配"]
    Model["local-model<br/>模型生命周期与推理"]
    Tools["工具策略与审批<br/>文件 / 搜索 / 受审批进程"]
    Native["类型化原生能力<br/>Windows / POSIX 适配"]
    Store[("EventStore · SQLite<br/>会话与执行状态真相源")]
    Cache["模型缓存<br/>GGUF / 独立决策 bundle"]
    Workspace["受控工作区<br/>文件与进程 cwd"]

    Client <--> Access <--> Boundary
    Boundary --> Runtime
    Boundary --> Native
    Runtime -->|"前置调用"| Decision
    Runtime -->|"记录决策后执行"| Pi
    Runtime -->|"先持久化，再投影"| Store
    Pi --> Model --> Cache
    Pi --> Tools --> Workspace
    Native --> Cache
    Native --> Workspace
  end

  subgraph Orchestration["任务编排与执行节点"]
    Root["Root authority<br/>RootPlanner / TaskGraph<br/>单写入权 / ownerEpoch"]
    Context["ContextSlice / WorkspaceManager<br/>必要上下文、固定基线与独立工作区"]
    Dispatch["Scheduler / Mesh<br/>确定性选点、设备身份与授权<br/>版本化任务契约"]
    Admission["NodeAdmission / Executor<br/>原子预留槽位、租约与 Attempt"]
    Execution["Subagent / Tool / Data<br/>独立会话、工作区与本地事件存储<br/>仅 agent 任务需要 LLM"]
    Results["Root Verifier<br/>结果 / 产物 / 证据<br/>验收后 accepted"]
    Center["可选 Center · optional<br/>registry / relay / backup"]

    Root --> Context --> Dispatch --> Admission --> Execution --> Results
    Center -.-> Dispatch
  end

  User --> Client
  Runtime --> Root
  Results --> Root
  Root --> Store

```

<details>
<summary>展开完整模块关系（44 个节点）</summary>

```mermaid
flowchart TB
  U["用户"]
  subgraph Local["本地节点 · Node.js 24 / strict TypeScript"]
    direction TB
    subgraph Clients["认证客户端 · 不持有执行资源"]
      direction LR
      TUI["TUI · agent-cli"]
      Renderer["Electron renderer<br/>view model / 无 Node API"]
      Preload["受限 preload<br/>getState / onState / act / reconnect"]
      Main["Electron main<br/>窗口与 worker 生命周期"]
      Worker["独立 Node 24 client worker"]
      Launch["agent-launch<br/>发现与必要时拉起"]
      Client["agent-client<br/>握手 / 认证 / 重连"]
      View["ui-model<br/>客户端状态投影"]
      Protocol["protocol<br/>版本化本地 gRPC 契约"]
    end
    subgraph Daemon["agentd · 唯一生产组合根"]
      direction LR
      Auth["gRPC 服务<br/>认证与边界校验"]
      FIFO["命令 FIFO<br/>预算 / epoch / 串行化"]
      ModelState["Model-aware Runtime<br/>模型状态与激活"]
      Runtime["agent-runtime<br/>持久执行与恢复"]
      Domain["agent-domain<br/>领域事件 / reducer"]
      Decision["DecisionProvider<br/>类型化前置决策 / 安全回退"]
      Policy["daemon 策略<br/>确定性策略约束动作"]
      Pi["kernel-pi<br/>Pi 执行引擎适配"]
      Model["local-model<br/>生命周期与推理"]
      Approval["工具授权与审批"]
      Tools["工作区文件 / 搜索 / 进程工具"]
      Control["local-control-plane<br/>scope / token / lease / discovery"]
      Native["类型化原生能力<br/>Windows / POSIX 与文件、进程适配"]
    end
    subgraph Resources["本节点由 daemon 持有或受控访问的资源"]
      direction LR
      Discovery["受保护 discovery<br/>descriptor / token / lease"]
      Store[("EventStore · SQLite WAL<br/>会话与执行状态真相源")]
      Weights["模型缓存<br/>GGUF / 独立决策模型 bundle"]
      Workspace["受控工作区<br/>文件与子进程 cwd"]
    end
  end
  subgraph Root["逻辑 Root 编排"]
    direction LR
    Authority["Root authority<br/>根会话单写入权 / ownerEpoch"]
    Planner["RootPlanner"]
    Graph["TaskGraph<br/>依赖 / 范围 / 验收条件"]
    Context["ContextSlice<br/>最小上下文与授权引用"]
    Workspaces["WorkspaceManager<br/>固定基线 / 独立工作区"]
    Scheduler["deterministic Scheduler"]
    Verifier["Root Verifier<br/>验收与集成"]
    Mesh["Mesh<br/>设备身份 / 授权 / 版本化任务契约"]
  end
  subgraph Remote["执行 Node"]
    direction LR
    Admission["NodeAdmission<br/>本机原子预留并发槽位"]
    Executor["Executor<br/>agent / tool / data 契约执行"]
    Subagent["Subagent<br/>独立 session / 完整 Runtime"]
    NodeStore[("节点本地 EventStore<br/>Attempt / 子会话 / 待回传结果")]
    TaskWorkspace["独立 workspace<br/>Git / 数据 / 工具资源"]
    Results["结果与产物<br/>commit / 日志 / 测试或来源证据"]
  end
  subgraph Center["可选 Center · optional"]
    direction LR
    Registry["registry · optional"]
    Relay["relay · optional"]
    Backup["backup · optional"]
  end

  U --> TUI
  U --> Renderer
  Renderer --> Preload --> Main --> Worker
  Worker --> Launch
  Worker --> Client
  TUI --> Launch
  TUI --> Client
  Launch --> Discovery
  Client <-->|"命令 / Snapshot"| Protocol
  Protocol <--> Auth
  Client --> View --> TUI
  Auth --> FIFO --> ModelState --> Runtime
  ModelState --> Auth
  Runtime --> Domain
  Runtime --> Decision --> Policy --> Pi
  Pi --> Model --> Weights
  Decision --> Weights
  Pi --> Approval --> Tools --> Native
  Control --> Auth
  Control --> Discovery
  Control --> Native
  Native --> Workspace
  Runtime -->|"先持久化领域事件"| Store
  Store -->|"恢复"| Runtime

  Runtime --> Authority
  Authority --> Planner --> Graph --> Scheduler
  Graph --> Context
  Graph --> Workspaces
  Context --> Mesh
  Workspaces --> TaskWorkspace
  Scheduler --> Mesh --> Admission --> Executor
  Executor -->|"agent"| Subagent
  Executor -->|"tool / data"| Results
  Subagent --> NodeStore
  Subagent --> TaskWorkspace
  Subagent --> Results --> Verifier --> Authority
  Authority --> Store
  Registry -.-> Mesh
  Relay -.-> Mesh
  NodeStore -.-> Backup
  Store -.-> Backup

```

</details>

上图中的客户端输出线作了简化：daemon 的 Snapshot stream 回到 `agent-client`；TUI 经 `ui-model` 渲染，Desktop 经 worker、main 与受限 preload 回到 renderer。Desktop main 管理窗口和 worker，由独立 Node 24 worker 复用 launcher/client 完成原生发现与认证，不把 native backend 装进 renderer。

`agentd` 是节点内的唯一组合根，持有 Runtime、模型、工具、审批、MCP 连接和 EventStore 的生命周期。Secret 与 Skills 接入受 daemon 的授权策略约束。Pi、OpenJev 等第三方类型止于适配层，不进入公共协议或客户端 view model。关闭客户端不等于取消 daemon 已接受的执行。

## 模块职责

### 节点执行与客户端

| 模块与代码位置                                                                                   | 输入 → 输出                                                    | 所有权与权限边界                                                                                           |
| ------------------------------------------------------------------------------------------------ | -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `apps/agent-cli`、`packages/ui-model`                                                            | 用户操作、Snapshot → 命令、终端投影                            | Client；不持有 Runtime 或数据库。                                                                          |
| `apps/agent-desktop/src/{renderer,preload,main}`                                                 | UI action、受限 IPC、worker 状态 → renderer view model         | Renderer 无 Node API/token；main 管理独立 Node 24 client worker，worker 复用公开 launcher/client。         |
| `packages/agent-launch`、`agent-client`、`protocol`                                              | workspace scope、发现信息、命令 → daemon 连接与版本化 Snapshot | Launcher 负责必要时启动；client 使用凭据认证，凭据不暴露给展示层；本地 gRPC 契约与节点间任务契约分别管理。 |
| `apps/agentd/node-src/composition.ts`、`production-runtime.ts`、`production-kernel.ts`           | 配置、scope、模型 lease → Runtime、Kernel、工具与 gRPC 组合    | 唯一生产组合根；负责资源创建与关闭。                                                                       |
| `apps/agentd/node-src/runtime-command-serial-queue.ts`、`packages/agent-runtime`、`agent-domain` | 校验后的命令、KernelEvent → 领域事件、恢复状态与 Snapshot      | 命令 FIFO 与 Runtime 持久化分别承担自己的顺序约束；不能用模型决策替代确定性校验。                          |
| `packages/kernel-pi`、`local-model`、`decision-engine`                                           | 模型输入、封闭问题、工具步骤 → 规范化执行结果、类型化决策      | Daemon 拥有生命周期；决策结果或回退先落库，再继续 Pi。                                                     |
| `apps/agentd/node-src/{workspace-tools,approval-gate}.ts`                                        | 有范围的工具请求、用户审批 → 文件/进程结果、审批事件           | 文件工具有工作区边界；`process_execute` 需高风险审批，以主机用户权限运行，不是 OS 沙箱。                   |
| `packages/local-control-plane`、`control-plane-native`、`event-store`                            | 原生能力请求、领域事件 → 安全发现/锁/模型存储、SQLite 事件     | Windows/POSIX 差异集中在适配层；文件工具另复用 `@openclaw/fs-safe`，进程由系统适配清理。                   |

### 任务编排与节点协作

| 逻辑模块                                | 输入 → 输出                                                       | 权威边界                                                                                                   |
| --------------------------------------- | ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| RootPlanner / TaskGraph                 | 用户目标、基线、约束 → 带依赖、范围、预算和验收条件的 Task        | Root 管理计划；有依赖的任务不能提前进入可调度状态。                                                        |
| ContextSlice / WorkspaceManager         | 任务与受授权引用 → 最小上下文、独立工作区与产物引用               | 子任务只获得所需内容；Coding 使用固定基线和独立 clone/worktree，不共享可写目录。                           |
| deterministic Scheduler / NodeAdmission | 就绪 Task、节点能力与负载 → Offer、原子接单与租约                 | Scheduler 确定性选候选节点；接收 Node 自行校验权限并预留本机资源。                                         |
| Mesh / 版本化任务契约                   | 候选地址、身份、授权、任务消息 → 可信连接与可核对消息             | 发现不代表已加入，加入不代表获执行权限；本地 gRPC/token 不能直接充当 Mesh 授权。                           |
| Executor / Subagent / Root Verifier     | Task、ContextSlice、授权 → submitted 结果与证据 → accepted 或退回 | 子会话和工作区独立；agent 任务使用完整 Runtime；tool/data 可按契约直接执行；只有 Root 验收后任务才算完成。 |
| 复制 / 主动交接 / 可选 Center           | 已授权事件、快照与产物 → 副本或明确的权威转移                     | 副本不是写入者；Center 可提供 registry、relay、backup，但不决定默认运行拓扑。                              |

## 单机命令、执行与持久化

单机执行以「先持久化领域事件，再发布投影」为顺序约束。初始化与模型状态变化也可生成 Snapshot；执行结果必须经过 EventStore 后才能发布。

```mermaid
sequenceDiagram
  autonumber
  actor User as 用户
  participant Client as 认证客户端
  participant Daemon as agentd 鉴权与 FIFO
  participant Runtime as agent-runtime
  participant Decision as DecisionProvider / daemon 策略
  participant Pi as kernel-pi / 模型与工具
  participant Store as EventStore

  User->>Client: 提交输入
  Client->>Daemon: 类型化命令 + session / commandId / expectedServerInstanceId
  Daemon->>Daemon: 鉴权、scope/预算校验、FIFO 串行化
  Daemon->>Runtime: 接受 start-turn
  Runtime->>Store: append user_message_submitted
  Store-->>Runtime: 持久化成功
  Runtime->>Decision: 受限问题与状态
  Decision-->>Runtime: 已校验结果或安全回退；continue_pi
  Runtime->>Store: append decision_recorded 或 decision_fallback_recorded
  Store-->>Runtime: 持久化成功
  Runtime->>Pi: startTurn
  loop 每个执行事件
    Pi-->>Runtime: 规范化 KernelEvent
    Runtime->>Runtime: 映射为 Lyonbrown 领域事件
    Runtime->>Store: append 领域事件
    Store-->>Runtime: 持久化成功
    Runtime-->>Daemon: 发布领域状态投影
    Daemon-->>Client: Snapshot + 单调 transport revision
    Client-->>User: 更新界面
  end
```

工具请求由 daemon 的确定性策略与审批门控制。需要审批时，客户端只显示请求并提交用户决定；拒绝决定同样进入领域事件，模型不能扩大授权。EventStore 写入失败时，Runtime 不把该执行结果投影为成功状态。客户端 Snapshot 的 transport revision 与持久事件 revision 各有用途，不能互换。

恢复从 EventStore 重建领域状态与可安全续接的历史，Pi 内部状态不是第二个权威存储。

## 任务分派与跨节点执行

Node 是承载资源的物理执行节点；Agent 是逻辑执行实体；Run 是根目标的一次执行；Task 是稳定工作单元；Attempt 是 Task 的一次实际尝试。重试创建新 `attemptId`，不重建根会话，也不覆盖旧尝试的证据。Executor 按 `ExecutionKind = agent | tool | data` 执行：只有 agent 任务需要启动 Subagent，确定的 tool/data 工作不强制调用 LLM，但都必须接受 Root 验收。

```mermaid
sequenceDiagram
  autonumber
  participant Root as Root authority
  participant RootStore as Root EventStore
  participant Scheduler as Scheduler / Mesh
  participant Admission as 远端 NodeAdmission
  participant Executor as Executor
  participant Agent as Subagent
  participant NodeStore as 远端本地 EventStore
  participant Verifier as Root Verifier

  Root->>RootStore: 保存 TaskGraph、范围、验收条件与 ownerEpoch
  Root->>Scheduler: 就绪 Task + ContextSlice / WorkspaceRef
  Scheduler->>Admission: TaskOffer：版本、身份、授权、Task/Attempt、幂等键
  Admission->>Admission: 校验契约、能力、预算与权限，原子预留本机槽位
  alt 拒绝或资源不足
    Admission-->>Scheduler: 拒绝原因；不启动 Attempt
  else 接受
    Admission->>NodeStore: 保存接单与租约状态
    Admission-->>Scheduler: Accept + lease
    Scheduler-->>Root: 已接单的 Node / Attempt / lease
    Root->>RootStore: 保存分配与 Attempt 状态
    Admission->>Executor: 启动已授权 Attempt
    alt agent 任务
      Executor->>Agent: 启动独立 session / workspace
      Agent->>NodeStore: 保存本地执行事件与产物引用
      Agent-->>Executor: 提交候选结果
    else tool / data 任务
      Executor->>Executor: 按任务契约直接执行，不强制启动 LLM
      Executor->>NodeStore: 保存执行结果与产物引用
    end
    Note over Executor,NodeStore: 断线时只继续仍获授权且可安全执行的步骤；本地保存待回传结果
    Executor-->>Root: 重连后提交 submitted 结果、产物与证据
    Root->>Root: 核对身份、ownerEpoch、Attempt、租约状态并去重
    Root->>Verifier: 检查产物、测试或来源证据与验收条件
    Verifier-->>Root: accepted 或退回修复
    Root->>RootStore: 持久化验收决定后推进 TaskGraph
  end
```

节点间任务契约需要版本化，并明确消息字段、失败窗口、重传和取消语义。图中的消息名用于说明契约职责。

每个 Node 只写自己的本地 EventStore；Subagent 不直接写 Root Session，也不与其他机器共享 SQLite 文件。Coding 的产物以 commit、补丁及测试日志交付；数据任务返回 Schema、数据分片、来源与校验信息。Root 只向子任务发送必要 ContextSlice，不默认复制完整对话、凭据或所有工作文件。

租约到期不能证明旧 Attempt 已停止，更不能证明外部副作用未发生。重新派发使用新 Attempt，并按副作用的幂等条件或人工复核决定是否重试；旧权威或失效尝试的提交不能直接验收。断线 Node 保存结果，恢复后按任务、尝试与事件标识去重回传；需要新的 Root 决策、无法确认授权或副作用安全性时暂停。

`ownerEpoch` 表示根会话写入权的版本，随明确交接递增。本地 daemon 代次由协议字段 `serverInstanceId` 表达，用于握手；命令中的 `expectedServerInstanceId` 必须与它匹配。daemon 重启不等于 Root 权威转移，二者不能复用。默认由发起节点持有 Root 权威；只有完成状态同步、目标确认和交接提交后，新节点才能继续根会话写入。断线或发现不到旧节点不触发自动接管。

Mesh 的职责是安全传输、设备身份与任务授权；底层传输和 LAN/VPN 候选发现机制由适配层选择。发现节点不等于建立信任，建立连接也不等于获得执行授权。Center 可提供 registry、relay 和 backup，但不能成为单机启动或节点直连的硬依赖。

## 跨平台能力与运行约束

文件、路径、权限、锁、进程、模型存储和工作区操作通过类型化能力接口提供。Windows、macOS、Linux 共用输入输出、授权、取消、超时和错误语义；系统差异集中在平台适配层，缺失能力必须明确报告。

Windows 原生 share-deny 与 POSIX advisory lock 使用不同机制。POSIX 锁协调本适配器的并发操作，不承诺阻止同一 OS 用户下恶意进程的写入。进程工具以主机用户权限运行；审批与工作区检查不构成 OS 沙箱。

后台业务调度若需要跨重启和重试状态，应先定义 job port，再选择受维护的持久化队列或工作流实现；不把原生 timer 或 EventStore 临时表当成调度器。

任务模型、上下文与恢复策略详见 [System One 决策层](/zh/architecture/system-one-decision-layer/)及相关架构页面；交付顺序与运行验收记录见[迭代路线](/zh/plans/roadmap/)和[发布制品](/zh/reference/release-artifacts/)。
