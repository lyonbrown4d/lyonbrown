# Local Models and Task Routing

## Embedded llama.cpp by default

`agent-cli` enables the `llama-cpp` feature by default, and the default `model.provider` is `llamacpp`. The model is loaded in the Agent process, with no Ollama installation or separate HTTP daemon required. Ollama and remote providers remain explicit configuration choices. A build made with `--no-default-features` reports a clear error if `llamacpp` is selected.

When `[llamacpp].model_path` is configured, the CLI uses that GGUF directly. Without a path, the model manager selects the largest built-in Qwen3 preset that fits the **currently available RAM**:

| Currently available RAM | Automatic preset | File |
|---:|---|---|
| 2–<4 GiB | `qwen3-0.6b-q8_0` | `Qwen3-0.6B-Q8_0.gguf` |
| 4–<8 GiB | `qwen3-1.7b-q4_k_m` | `Qwen3-1.7B-Q4_K_M.gguf` |
| 8–<14 GiB | `qwen3-4b-q4_k_m` | `Qwen3-4B-Q4_K_M.gguf` |
| ≥14 GiB | `qwen3-8b-q4_k_m` | `Qwen3-8B-Q4_K_M.gguf` |

This is available memory reported by the operating system at startup, not total physical RAM, and VRAM is not a separate selection input. Startup fails below 2 GiB. Set `[model].model` to one of the preset IDs to pin it. If available RAM is below that preset's threshold, startup still fails instead of silently downgrading.

## Download, cache, and integrity

Automatic models are downloaded from Qwen repositories on Hugging Face through `hf-hub` and installed in `~/.agent/models/`. The flow is:

1. Verify that an existing cache entry is a regular file and its SHA-256 matches the built-in catalog; reuse it if valid.
2. Check remote metadata before transfer and count every streamed chunk, so missing or misleading `Content-Length` headers cannot bypass the preset's hard byte limit.
3. Compute SHA-256 after download; remove staging and fail on mismatch.
4. Move it to a random `.part` path on the same filesystem, then atomically rename it to the final GGUF; validate it again on later startups.

The first startup therefore needs network access and sufficient disk space. A corrupt or mismatched cache entry is not loaded and is downloaded again; checksum failure never falls back to an unverified file. The current catalog uses `revision = "main"` while pinning acceptable content by SHA-256. If upstream replaces a file, the catalog digest must be reviewed and updated in code.

## Native Windows prerequisites

A Windows CPU build needs:

- Visual Studio Build Tools with the MSVC C++ toolchain and Windows SDK;
- CMake and Ninja;
- LLVM/Clang with `libclang.dll` discoverable by the bindings generator; set `LIBCLANG_PATH` when needed.

The repository provides `./scripts/build-windows.ps1`. It discovers Visual Studio and LLVM and sets `LIBCLANG_PATH` plus the Ninja CMake generator only for the current process; it does not write global `CC` or `CXX`. Use that script or `cargo build -p agent-cli` for the default CPU build. Enable the `cuda` or `vulkan` feature for those backends and also install the matching SDK, driver, and toolchain prerequisites. `gpu_layers = 0` still selects the CPU path.

## Typed task routing

Routing does not ask a control model for a free-form plan. `TaskRouter` accepts a typed `StepContext` and returns `Complexity::{Low, Medium, High}`. The context also carries tool/JSON capability requirements, previous-step count, escalation state, data classification, and any provider/model explicitly selected by the user. The Scheduler then uses those fields to filter capability, privacy, and cost constraints and choose a Control, Worker, or Frontier tier.

The embedded Qwen3 model can also serve as `ModelTaskRouter`. It receives a request with `temperature = 0`, at most 8 tokens, and thinking disabled, and may return only `LOW`, `MEDIUM`, or `HIGH`. If inference fails or the output is invalid, the Scheduler logs a warning and uses its deterministic heuristic classifier, keeping the router from becoming a single point of failure. “Typed” here means a Rust interface and a finite enum result, not an arbitrary JSON-schema decision API.

The current llama.cpp Rust API does not expose chat-template keyword arguments, so `thinking = false` appends Qwen3's official `/no_think` directive to the last user message before template rendering. Tool use follows a bounded `<tool_call>{...}</tool_call>` text protocol. The Provider accepts only one declared tool with object arguments that pass its JSON Schema, then converts it into a typed `ModelEvent::ToolCall`; ordinary answers remain text events.

## Why Laya is not yet a standard llama.cpp provider

Laya targets low-latency typed decisions such as choice, score, and noul rather than standard autoregressive chat completion. The current community GGUF conversion contains only the ModernBERT backbone; its decision head and configuration remain in a separate `safetensors` file and must execute outside llama.cpp. Lyonbrown's generic `LlamaCppProvider`, by contrast, expects a GGUF with usable `tokenizer.chat_template` metadata and produces a stream of text tokens.

Loading Laya's backbone in llama.cpp therefore does not satisfy Lyonbrown's `ModelProvider` contract. Laya is a better candidate for a dedicated `TaskRouter`/typed-decision adapter with separate validation for the head, tokenizer, schema, calibration, and backend parity. Until that exists, it is not presented as a standard llama.cpp provider. See [laya.cpp](https://github.com/lkarlslund/laya.cpp) and the [community GGUF architecture notes](https://huggingface.co/fr0stbit3/laya-gguf).
