# 本地模型与任务路由

> 已验证实现检查点：2026-10-07。composition 与有界 inference 路径已有聚焦测试
> 覆盖；当前 Windows 开发机上的生产 composition 真实 GGUF smoke 已通过。发布打包
> 与最终 release artifact 仍未验证。

## Node daemon 默认持有 llama.cpp

Lyonbrown 只有一种产品架构：`agentd` 持有 Runtime、模型、工具、MCP 和 Session，TUI 是经过认证的 gRPC client。客户端不加载 GGUF，也没有 embedded Runtime 回退。

Node 实现使用精确锁定的 `node-llama-cpp`，并将其作为必需依赖随产品交付。发布包只使用验证过的预编译原生二进制；运行时设置 `build: "never"` 与 `skipDownload: true`，禁止首次启动时静默编译 llama.cpp 或下载原生组件。原生二进制缺失或不兼容时必须返回结构化不可用状态。

daemon 内置一个有界、可信的候选目录，而不是写死单一默认模型：

| Preset              | 量化   | 固定制品大小    | 保守内存下限 | 许可证     |
| ------------------- | ------ | --------------- | ------------ | ---------- |
| `qwen3-0.6b-q8_0`   | Q8_0   | 639,446,688 B   | 2 GiB        | Apache-2.0 |
| `qwen3-1.7b-q4_k_m` | Q4_K_M | 1,107,408,544 B | 4 GiB        | Apache-2.0 |
| `qwen3-4b-q4_k_m`   | Q4_K_M | 2,497,280,256 B | 8 GiB        | Apache-2.0 |

每个候选的仓库、文件名、revision、SHA-256、大小上限、最低上下文和许可证都固定在
代码中，不使用 `main`，也不接受 client 提供的 URL、目标路径或摘要。新增或升级候选
必须经过代码审查、摘要更新、许可证检查、真实硬件评估和 GGUF smoke test。

## 硬件感知推荐

推荐不是一个新的模型下载器，也不是一组手写的 GPU 型号规则。daemon 在后台通过
`node-llama-cpp` 读取本机 RAM/VRAM 状态和可信 GGUF 元数据，并使用其
`GgufInsights` 配置解析器对固定 4096-token 上下文评分。`node-llama-cpp` 的具体类型
只存在于 local-model 适配层；gRPC 和 TUI 只看到 Lyonbrown 自有的模型描述与有限推荐
原因。

选择顺序固定为：

1. 优先复用目录中已经安装且摘要验证通过的模型；离线启动不会因为无法远程评估而
   要求重新下载另一个模型。
2. 没有已安装模型时，只对固定大小预算内的可信候选做硬件兼容性评分；当前首次下载
   预算为 1 GiB，因此默认保持小模型，不会因瞬时空闲内存跳到多 GiB 下载。
3. 硬件探测或远程 GGUF 元数据读取失败时，确定性回退到目录中最小的可信候选。
4. 推荐完成后只发布 `setup-required` 和允许的动作；下载仍需用户在 TUI 中显式确认。

该流程不会把原始 RAM、VRAM、GPU 型号或探测错误发给 client，也不会让模型目录外的
ID 或 URI 参与评分。推荐请求有界、可取消；失败降级不能扩大网络或文件系统权限。

## 非阻塞启动与显式下载

打开 TUI 或启动 daemon 不会自动下载模型。缓存检查在后台执行，界面可以立即显示连接和模型设置状态。模型缺失时禁止提交依赖该模型的 turn，但用户仍可查看状态、修改配置或退出。

下载只能由用户显式触发。daemon 投影以下阶段：

`checking → setup-required → waiting-for-lock → downloading → verifying → installing → ready`

取消或失败分别进入 `cancelled` 和 `failed`。每次操作都有新的 `operation_id`；旧 ID 不能取消新任务。进入原子安装阶段后取消返回 TooLate。任务属于 daemon，关闭 TUI 不会隐式取消下载。进度与结果进入现有的版本化 `RuntimeSnapshot`；TUI 不实现下载器。

模型激活与下载是两个阶段。已验证的缓存模型可以进入 `activating`；激活失败后可以
显式重试，而不会启动新的下载。激活期间即使没有 download `operation_id`，TUI 也会
保持 chat 不可用。仅模型状态更新与 Runtime 更新共享 daemon 的单调 transport
revision。

下载实现使用 Node 24 内建 `fetch` 将 response body 流式写入原生 staging。请求、响应
body 与文件写入共享取消信号；只有响应 headers 到达前的瞬时请求失败会执行有限次数
重试。生产 composition 通过 `setGlobalProxyFromEnv` 配置 Node 全局代理，并且只传入
`HTTP_PROXY`、`HTTPS_PROXY`、`NO_PROXY` 及其小写形式；认证 loopback gRPC 明确禁用
环境代理发现。

## 安全与完整性边界

生产组合必须由 native provider 注入安全模型存储能力，并满足：

- 缓存目录和文件由当前用户独占，拒绝 junction、symlink/reparse point 与身份漂移；
- 使用不可预测的同文件系统 staging，超过字节上限时立即中止；
- 固定 SHA-256 验证通过后才允许原子激活；
- 从摘要校验到 `node-llama-cpp` 完成加载期间保持稳定文件锚点，拒绝写入、删除和替换；
- 使用可由内核观察的跨进程安装锁，等待者获取锁后重新检查缓存；
- daemon 关闭会取消并有界等待检查、下载和模型加载，迟到的原生 lease 必须释放。

缺少这些 capability 时必须 fail closed，不能退化到普通 `homedir()` 路径操作。模型路径、下载 URL、摘要、原生句柄和底层 `node-llama-cpp` 对象不能进入公共 gRPC/UI contract。

## 当前实施状态

`packages/local-model` 已实现托管 preset、后台检查/下载、跨进程锁、摘要验证、原子
激活、原生加载和 Lyonbrown 自有的流式 inference lease。Windows 安全存储适配器与
服务已接入生产 `agentd` composition root；TUI 仍只是认证 client。

默认小模型 lease 只允许一个活跃 inference。第二个请求会立即以 `resource_busy`
失败，不排队，也不会创建第二个 context。被接受的请求在固定 4096-token context
中使用一个 sequence，传递取消信号，并限制消息数、总输入、输出 token、输出字节、
事件队列与关闭清理。原生错误会脱敏；工具定义、tool-call 与 tool-result history 在
适配层映射并接受严格边界校验。Qwen3 inference 固定向 `node-llama-cpp` 传入
`budgets.thoughtTokens=0`，避免
隐藏 thought 用尽 client 可见的输出 token 预算。

daemon 命令入口将每条 user、follow-up 或 steering 消息限制为 64 KiB，与
local-model 的单消息上限一致。生产 Pi 适配器的 256 KiB 限制是完整构造后 inference
总输入的防御上限，不是单条消息配额。非活跃 Session 会在下一次输入前于 32 个历史
turn 或 4 MiB 用户文本处轮换；活跃 turn 的 follow-up 限制为 8 条、合计 512 KiB。
预算检查与持久化在 daemon 的同一条 FIFO 命令通道中执行，避免并发 RPC 绕过这些
限制。daemon 重启后会从持久领域状态重建有界的已完成历史并交给 Pi。包含 steering、
待审批、未决工作、结果不确定、非法 phase、超过 128 条重建消息或超过 256 KiB 历史的
Session 会安全轮换；持久事件仍保留在 EventStore。

当前 Windows 开发机上的生产 composition 真实 GGUF smoke 会读取实际硬件选择结果，
再从同一可信目录校验固定摘要，而不是在脚本中写死模型。2026-10-07 的运行验证了：
启动不自动下载；显式 operation 的取消/重试、进度、摘要校验与激活；pinned/auto 模型
选择；client 可见回答和 usage；真实 `write` tool-call、待审批投影、`allow-once` 与准确
落盘；长 turn 取消后继续；缓存重启不重新下载并创建新 Session；以及 30 秒预算内的
有界关闭。脚本输出 `REAL_GGUF_SMOKE_OK`，并清理了临时模型缓存。

以上只是当前 Windows 开发机和生产 composition 的执行证据，不是发布打包或最终
release artifact 的验证。GUI、Desktop、移动端与 MCP 仍不在本阶段范围内。

## 类型化任务路由

任务路由位于 daemon 内。输入是包含能力、隐私、预算和用户显式选择的类型化步骤上下文，输出是有限的复杂度/模型层级，而不是自由文本计划。Qwen3 工具调用适配已经接入；更完整的多模型任务路由仍需迭代。路由失败必须有明确 fallback，不能让控制模型成为单点故障。

当前生产 Kernel 使用受工作区约束的 `read`、`write`、`edit` registry 与显式审批门；
文件能力由 native-required 的 `@openclaw/fs-safe` 根对象提供；如果可信目录中没有模型
满足 1 GiB 首次自动下载预算，选择流程会 fail closed。Secret resolver 仍拒绝，MCP 尚未
连接。模型文本是不受信任的输出，不会自行获得工具权限。GUI、Desktop 与移动端仍不在
当前迭代范围内。
