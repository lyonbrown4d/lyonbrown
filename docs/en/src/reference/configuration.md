# Configuration Reference

Lyonbrown uses `config-rs` to layer TOML, YAML, JSON, JSON5, RON, and INI. Precedence from lowest to highest is: built-in defaults, `~/.agent/config.toml`, workspace `agent.toml`, a `--config` file, `LYONBROWN_` environment variables, and CLI flags. Nested environment keys use double underscores, for example `LYONBROWN_MODEL__PROVIDER=ollama`.

## Ollama

```toml
[model]
provider = "ollama"
model = "qwen3.8-27b:q3-claude"
max_tokens = 2048

[ollama]
endpoint = "http://127.0.0.1:11434"
```

Run `agent ask "hello"`, `agent chat`, or `agent .` inside a workspace to open the TUI. The TUI and headless CLI share the same Kernel, event stream, and approval policy.

## Embedded llama.cpp

The llama.cpp provider runs in process: the Agent loads GGUF directly, with no HTTP daemon or inference subprocess. Native dependencies are disabled by default. Enable the CPU backend with `--features llama-cpp`; the `cuda` and `vulkan` features are optional.

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

Relative model paths are resolved from the workspace. `LYONBROWN_LLAMACPP__MODEL_PATH` is the equivalent environment setting. Native Windows builds require MSVC, CMake/Ninja, and an LLVM `libclang` discoverable by the bindings generator; set `LIBCLANG_PATH` when necessary.

```powershell
cargo run -p agent-cli --features llama-cpp -- --config agent.toml ask "Introduce Rust in one sentence"
```

Selecting `llamacpp` without the feature produces an explicit error instead of falling back to an external service. For real-model acceptance, set `LYONBROWN_GGUF_MODEL` and run the provider's ignored live smoke test.
