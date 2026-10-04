# 本地模型与任务路由

## 默认由 daemon 持有 llama.cpp

`agentd` 默认启用 `llama-cpp` 能力，默认 `model.provider` 也是
`llamacpp`。GGUF 只在 daemon 进程内加载；daemon 是模型、Kernel、工具和
Session 状态的唯一所有者。TUI 与 GUI 都是经过认证的 gRPC client，不加载模型，
也不保留 embedded Runtime 回退。Ollama 和远程 Provider 仍可通过配置显式选择。
若 `agentd` 使用 `--no-default-features` 构建，选择 `llamacpp` 会明确报错。

当 `[llamacpp].model_path` 已配置时，`agentd` 直接使用该 GGUF；若配置路径
不存在或不是普通文件，也会发布结构化 `ModelSetupRequired` 状态，而不是在写入
discovery 前退出。未配置路径时，默认 preset 是约 1.1 GB 的
`qwen3-1.7b-q4_k_m`，daemon 会先检查经过
校验的本地缓存。模型缺失时，Runtime 进入结构化 `ModelSetupRequired` 阶段，
不会在启动过程中下载；client 仍可进入界面，但在模型设置完成前不可提交任务。
可将 `[model].model` 设为其他内置 preset ID 覆盖默认值：

| Preset                      | 下载大小 | 最低可用 RAM | 文件                     |
| --------------------------- | -------: | -----------: | ------------------------ |
| `qwen3-0.6b-q8_0`           |  0.64 GB |        2 GiB | `Qwen3-0.6B-Q8_0.gguf`   |
| `qwen3-1.7b-q4_k_m`（默认） |  1.11 GB |        4 GiB | `Qwen3-1.7B-Q4_K_M.gguf` |
| `qwen3-4b-q4_k_m`           |  2.50 GB |        8 GiB | `Qwen3-4B-Q4_K_M.gguf`   |
| `qwen3-8b-q4_k_m`           |  5.03 GB |       14 GiB | `Qwen3-8B-Q4_K_M.gguf`   |

内存保护使用启动时操作系统报告的可用内存，不是物理内存总量，也不单独使用 VRAM。若当前可用 RAM 低于所选 preset 的门槛，启动会明确失败，不会静默更换用户选择。

## 模型设置与后台下载

仅仅打开 `agent` 或 GUI 不会让 client 或 daemon 发起网络传输。已配置的 GGUF
或通过校验的缓存可以直接使用；否则 `agentd` 通过正常的 gRPC 状态流发布
`ModelSetupRequired`。

daemon 后台下载能力已经落地，但只能由 client 显式调用 unary
`StartModelDownload` 开始；启动和仅仅进入界面绝不触发网络请求。取消使用 unary
`CancelModelDownload`，下载进度与结果进入现有的单一版本化
`RuntimeSnapshot` stream，不另建模型专用 stream。TUI/GUI 现已在同一组
Snapshot/RPC 上提供显式 `Download`、`Retry` 和 `Cancel` 操作项，两端都不自行
实现下载器。两端支持英文与简体中文，在 daemon 任务运行时保持可用，保留失败
详情，并在 Snapshot 过期时禁用 daemon 操作。GUI 设置卡片位于 conversation 与
Composer 之间，Runtime 状态栏同步显示真实 phase 和进度。

托管模型的 Snapshot 带有 source、phase、allowed actions、字节进度、failure 与
`operation_id`。phase 包括 `WaitingForLock`、`Checking`、`Downloading`、
`Verifying`、`Installing`、`Activating`、`Ready`、`Cancelled` 和 `Failed`。
下载任务属于 daemon，关闭 client 不会取消它；Cancel 必须引用当前 operation，
旧 operation ID 不得影响新任务。进入 `Installing` 后取消会返回 TooLate。失败后
可以 Retry；因为没有断点续传，该动作不称为 resume。

client 只能请求当前配置对应的内置 preset ID，不能提交下载 URL、目标路径或摘要。
显式配置的 `model_path` 缺失时来源是 configured path，不提供 Download 动作。托管
下载的完整性契约为：

1. 先验证已有缓存是否为普通文件且 SHA-256 与内置目录一致；有效则复用。
2. 下载前检查远端元数据；下载时逐 chunk 累计字节数，因此缺失或伪造 `Content-Length` 也不能绕过每个 preset 的硬上限。
3. 下载完成后计算 SHA-256；不匹配则删除 staging 并失败。
4. 在同一文件系统内写入 staging，再原子持久化为最终 GGUF；下次启动再次校验。

下载始终是可选步骤：用户可以不发起任何网络请求，直接离开设置。损坏或
版本不匹配的缓存不会被加载；校验失败也不会降级为未经验证的文件。当前目录
固定 `revision = "main"`，SHA-256 固定了可接受内容；上游替换文件时，需要先
在代码中审查并更新目录摘要。

安装成功后由 `agentd` 的 in-process epoch supervisor 构建 replacement Runtime。
旧 control plane 在构建期间保持在线；新 endpoint 启动并原子发布 descriptor 后
才切走旧 endpoint。地址、token 和 server UUID 会轮换，scope、PID 与生命周期锁
保持不变；client 自动 reconnect 并从 Snapshot resync。构建或切换失败时保留旧
control plane 并发布可重试失败，不暴露半初始化 Runtime。

## Windows 原生构建

Windows CPU 构建需要：

- Visual Studio Build Tools（MSVC C++ 工具链和 Windows SDK）；
- CMake 与 Ninja；
- LLVM/Clang，且绑定生成器能找到 `libclang.dll`；必要时设置 `LIBCLANG_PATH`。

仓库提供 `./scripts/build-windows.ps1`，它会发现 Visual Studio 与 LLVM，仅在当前进程设置 `LIBCLANG_PATH` 和 Ninja CMake generator，不写入全局 `CC`/`CXX`。wrapper 会扫描中断后只留下 `CMakeCache.txt`、但未生成 Ninja/Visual Studio project 的 `llama-cpp-sys-2` 构建目录，并使用已锁定依赖的可信源码在原目录补跑 CMake generation；默认不会清理或另建整个 `target`。默认 CPU 版本使用该脚本构建 workspace，其中包括互为同级可执行文件的 `agent` 与 `agentd`；CUDA 或 Vulkan 后端分别启用 `cuda` 或 `vulkan` feature，并同时满足相应 SDK、驱动和工具链要求。`gpu_layers = 0` 仍表示 CPU 路径。

## 类型化任务路由

路由不是让控制模型生成自由文本计划。`TaskRouter` 接收类型化的 `StepContext`，返回 `Complexity::{Low, Medium, High}`。`StepContext` 还携带工具/JSON 能力要求、既往步骤数、是否升级、数据分类，以及用户显式指定的 Provider/Model。Scheduler 再用这些字段过滤能力、隐私和成本约束，并选择 Control、Worker 或 Frontier tier。

由 daemon 持有的 Qwen3 同时可作为 `ModelTaskRouter`：它以 `temperature = 0`、最多 8 token、关闭 thinking 的请求，只允许输出 `LOW`、`MEDIUM` 或 `HIGH`。输出无效或推理失败时，Scheduler 记录警告并使用确定性启发式分类，不让路由器成为单点故障。这里的“类型化”指 Rust 接口和有限枚举结果；它不是任意 JSON schema 的决策 API。

Qwen3 的当前 llama.cpp Rust API 不暴露 chat-template kwargs，因此 `thinking = false` 会在模板渲染前对最后一条用户消息追加官方 `/no_think` 指令。工具调用使用有界的 `<tool_call>{...}</tool_call>` 文本协议；Provider 只接受已声明的工具、对象参数和通过 JSON Schema 校验的单一调用，然后转换为类型化 `ModelEvent::ToolCall`。普通回答仍作为文本事件返回。

## 为什么 Laya 还不是标准 llama.cpp Provider

Laya 面向 choice、score、noul 等低延迟类型化决策，而不是标准自回归聊天补全。当前社区 GGUF 转换只包含 ModernBERT backbone；决策 head 与配置位于单独的 `safetensors` 文件中，必须在 llama.cpp 之外执行。相比之下，Lyonbrown 的通用 `LlamaCppProvider` 期望一个带可用 `tokenizer.chat_template` 的 GGUF，并产出文本 token 流。

因此，仅能被 llama.cpp 加载 backbone 不等于符合 Lyonbrown 的 `ModelProvider` 合约。Laya 更适合未来实现专用 `TaskRouter`/typed-decision adapter，并为 head、tokenizer、schema、校准和各后端结果建立独立验收；在这些条件满足前，不把它宣传为标准 llama.cpp Provider。参考 [laya.cpp](https://github.com/lkarlslund/laya.cpp) 与 [社区 GGUF 的架构说明](https://huggingface.co/fr0stbit3/laya-gguf)。
