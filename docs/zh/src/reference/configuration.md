# 配置参考

Lyonbrown 使用 `config-rs` 分层加载 TOML、YAML、JSON、JSON5、RON 和 INI。优先级从低到高为：内置默认值、`~/.agent/config.toml`、工作区 `agent.toml`、`--config` 指定文件、`LYONBROWN_` 环境变量、CLI 参数。嵌套环境变量用双下划线，例如 `LYONBROWN_MODEL__PROVIDER=ollama`。

## Ollama

```toml
[model]
provider = "ollama"
model = "qwen3.8-27b:q3-claude"
max_tokens = 2048

[ollama]
endpoint = "http://127.0.0.1:11434"
```

运行 `agent ask "你好"`、`agent chat`，或在工作区执行 `agent .` 打开 TUI。TUI 与 headless CLI 使用同一 Kernel、事件流和审批策略。

## Embedded llama.cpp

llama.cpp 是进程内 provider：Agent 直接加载 GGUF，不需要 HTTP daemon，也不会启动外部推理进程。原生依赖默认不编译；用 `--features llama-cpp` 启用 CPU 后端，`cuda` 或 `vulkan` feature 可选。

```toml
[model]
provider = "llamacpp"
model = "qwen-local"
max_tokens = 1024

[llamacpp]
model_path = "models/qwen.gguf"
context_size = 8192
max_output_tokens = 2048
batch_size = 512
threads = 8
batch_threads = 8
gpu_layers = 0
temperature = 0.7
seed = 0
```

相对模型路径以工作区为基准。也可以设置 `LYONBROWN_LLAMACPP__MODEL_PATH`。Windows 原生构建需要 MSVC、CMake/Ninja 和可被绑定生成器发现的 LLVM `libclang`；必要时设置 `LIBCLANG_PATH`。

```powershell
cargo run -p agent-cli --features llama-cpp -- --config agent.toml ask "用一句话介绍 Rust"
```

不带 feature 却选择 `llamacpp` 时，CLI 会明确报错，不会回退到外部服务。真实模型验收可设置 `LYONBROWN_GGUF_MODEL` 后运行 provider 中标记为 ignored 的 live smoke test。
