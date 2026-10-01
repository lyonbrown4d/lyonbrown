# 本地模型与任务路由

## 默认内嵌 llama.cpp

`agent-cli` 默认启用 `llama-cpp` feature，默认 `model.provider` 也是 `llamacpp`。模型在 Agent 进程内加载，不要求 Ollama 或独立 HTTP daemon；Ollama 和远程 Provider 仍可通过配置显式选择。若使用 `--no-default-features` 构建，选择 `llamacpp` 会明确报错。

当 `[llamacpp].model_path` 已配置时，CLI 直接使用该 GGUF。未配置路径时，TUI 默认推荐约 1.1 GB 的 `qwen3-1.7b-q4_k_m`。它会先检查经过校验的本地缓存，任何网络传输都必须由用户确认。可将 `[model].model` 设为其他内置 preset ID 覆盖默认推荐：

| Preset | 下载大小 | 最低可用 RAM | 文件 |
|---|---:|---:|---|
| `qwen3-0.6b-q8_0` | 0.64 GB | 2 GiB | `Qwen3-0.6B-Q8_0.gguf` |
| `qwen3-1.7b-q4_k_m`（默认） | 1.11 GB | 4 GiB | `Qwen3-1.7B-Q4_K_M.gguf` |
| `qwen3-4b-q4_k_m` | 2.50 GB | 8 GiB | `Qwen3-4B-Q4_K_M.gguf` |
| `qwen3-8b-q4_k_m` | 5.03 GB | 14 GiB | `Qwen3-8B-Q4_K_M.gguf` |

内存保护使用启动时操作系统报告的可用内存，不是物理内存总量，也不单独使用 VRAM。若当前可用 RAM 低于所选 preset 的门槛，启动会明确失败，不会静默更换用户选择。

## 下载、缓存与完整性

用户在 TUI 中确认后，模型才会通过后台任务从 Qwen 的 Hugging Face 仓库下载，并缓存到 `~/.agent/models/`。设置界面在下载时保持响应，显示字节数和百分比，并支持取消。`agent chat` 等 headless 命令绝不会隐式发起下载；它们只使用已配置或已校验的缓存，否则返回可操作的提示。流程为：

1. 先验证已有缓存是否为普通文件且 SHA-256 与内置目录一致；有效则复用。
2. 下载前检查远端元数据；下载时逐 chunk 累计字节数，因此缺失或伪造 `Content-Length` 也不能绕过每个 preset 的硬上限。
3. 下载完成后计算 SHA-256；不匹配则删除 staging 并失败。
4. 在同一文件系统内移动到随机 `.part` 路径，再原子重命名为最终 GGUF；下次启动再次校验。

下载不是强制步骤：用户可以不发起任何网络请求，直接退出设置。损坏或版本不匹配的缓存不会被加载；重新下载仍需用户明确确认。校验失败不会降级为未经验证的文件。当前目录固定 `revision = "main"`，SHA-256 固定了可接受内容；上游替换文件时，需要先在代码中审查并更新目录摘要。

## Windows 原生构建

Windows CPU 构建需要：

- Visual Studio Build Tools（MSVC C++ 工具链和 Windows SDK）；
- CMake 与 Ninja；
- LLVM/Clang，且绑定生成器能找到 `libclang.dll`；必要时设置 `LIBCLANG_PATH`。

仓库提供 `./scripts/build-windows.ps1`，它会发现 Visual Studio 与 LLVM，仅在当前进程设置 `LIBCLANG_PATH` 和 Ninja CMake generator，不写入全局 `CC`/`CXX`。默认 CPU 版本使用该脚本或 `cargo build -p agent-cli`；CUDA 或 Vulkan 后端分别启用 `cuda` 或 `vulkan` feature，并同时满足相应 SDK、驱动和工具链要求。`gpu_layers = 0` 仍表示 CPU 路径。

## 类型化任务路由

路由不是让控制模型生成自由文本计划。`TaskRouter` 接收类型化的 `StepContext`，返回 `Complexity::{Low, Medium, High}`。`StepContext` 还携带工具/JSON 能力要求、既往步骤数、是否升级、数据分类，以及用户显式指定的 Provider/Model。Scheduler 再用这些字段过滤能力、隐私和成本约束，并选择 Control、Worker 或 Frontier tier。

内嵌 Qwen3 同时可作为 `ModelTaskRouter`：它以 `temperature = 0`、最多 8 token、关闭 thinking 的请求，只允许输出 `LOW`、`MEDIUM` 或 `HIGH`。输出无效或推理失败时，Scheduler 记录警告并使用确定性启发式分类，不让路由器成为单点故障。这里的“类型化”指 Rust 接口和有限枚举结果；它不是任意 JSON schema 的决策 API。

Qwen3 的当前 llama.cpp Rust API 不暴露 chat-template kwargs，因此 `thinking = false` 会在模板渲染前对最后一条用户消息追加官方 `/no_think` 指令。工具调用使用有界的 `<tool_call>{...}</tool_call>` 文本协议；Provider 只接受已声明的工具、对象参数和通过 JSON Schema 校验的单一调用，然后转换为类型化 `ModelEvent::ToolCall`。普通回答仍作为文本事件返回。

## 为什么 Laya 还不是标准 llama.cpp Provider

Laya 面向 choice、score、noul 等低延迟类型化决策，而不是标准自回归聊天补全。当前社区 GGUF 转换只包含 ModernBERT backbone；决策 head 与配置位于单独的 `safetensors` 文件中，必须在 llama.cpp 之外执行。相比之下，Lyonbrown 的通用 `LlamaCppProvider` 期望一个带可用 `tokenizer.chat_template` 的 GGUF，并产出文本 token 流。

因此，仅能被 llama.cpp 加载 backbone 不等于符合 Lyonbrown 的 `ModelProvider` 合约。Laya 更适合未来实现专用 `TaskRouter`/typed-decision adapter，并为 head、tokenizer、schema、校准和各后端结果建立独立验收；在这些条件满足前，不把它宣传为标准 llama.cpp Provider。参考 [laya.cpp](https://github.com/lkarlslund/laya.cpp) 与 [社区 GGUF 的架构说明](https://huggingface.co/fr0stbit3/laya-gguf)。
