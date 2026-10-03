# Local Models and Task Routing

## Daemon-owned llama.cpp by default

`agentd` enables the `llama-cpp` capability by default, and the default
`model.provider` is `llamacpp`. The daemon loads the GGUF in its own process and
is the only owner of model, Kernel, tool, and session state. The TUI and GUI are
authenticated gRPC clients; neither client loads a model or keeps an embedded
Runtime fallback. Ollama and remote providers remain explicit configuration
choices. A daemon built with `--no-default-features` reports a clear error when
`llamacpp` is selected.

When `[llamacpp].model_path` is configured, `agentd` uses that GGUF directly.
If the configured path is absent or is not a regular file, the same structured
`ModelSetupRequired` phase is published instead of terminating before discovery.
Without a path, the default preset is `qwen3-1.7b-q4_k_m` (about 1.1 GB), and
the daemon checks the verified local cache. If the model is absent, the Runtime
enters the structured `ModelSetupRequired` phase instead of downloading during
startup. The client can still enter its UI, but submission stays unavailable
until model setup is resolved. Set `[model].model` to another built-in preset ID
to override the default:

| Preset | Download | Minimum available RAM | File |
|---|---:|---:|---|
| `qwen3-0.6b-q8_0` | 0.64 GB | 2 GiB | `Qwen3-0.6B-Q8_0.gguf` |
| `qwen3-1.7b-q4_k_m` (default) | 1.11 GB | 4 GiB | `Qwen3-1.7B-Q4_K_M.gguf` |
| `qwen3-4b-q4_k_m` | 2.50 GB | 8 GiB | `Qwen3-4B-Q4_K_M.gguf` |
| `qwen3-8b-q4_k_m` | 5.03 GB | 14 GiB | `Qwen3-8B-Q4_K_M.gguf` |

The memory guard uses available memory reported by the operating system at startup, not total physical RAM, and VRAM is not a separate input. If available RAM is below the selected preset's threshold, startup fails instead of silently changing the user's choice.

## Model setup and background download

No client or daemon starts a network transfer merely because `agent` or the GUI
was opened. A configured GGUF or a verified cache entry can be used immediately;
otherwise `agentd` publishes `ModelSetupRequired` over the normal gRPC state
stream.

Daemon-side background download is implemented, but it starts only after an
explicit unary `StartModelDownload` call. Startup and simply entering a UI
never initiate network traffic. Cancellation uses unary
`CancelModelDownload`; progress and results use the existing single versioned
`RuntimeSnapshot` stream rather than a model-specific stream. TUI and GUI now
provide the same explicit Download, Retry, and Cancel controls over those
Snapshot/RPCs; neither implements a downloader. Both surfaces support English
and Simplified Chinese, remain usable while the daemon job runs, keep failures
visible, and disable actions when their Snapshot is stale. The GUI setup card
sits between the conversation and Composer, and its Runtime rail mirrors the
actual phase and progress.

The managed-model Snapshot carries its source, phase, allowed actions, byte
progress, failure, and `operation_id`. Phases include `WaitingForLock`,
`Checking`, `Downloading`, `Verifying`, `Installing`, `Activating`, `Ready`,
`Cancelled`, and `Failed`. The job belongs to the daemon, so closing a client
does not cancel it. Cancel must reference the current operation; a stale
operation ID cannot affect a newer job. Cancellation returns TooLate after
`Installing` begins. A failure permits Retry. It is not called resume because
partial transfers are not resumed.

Clients can request only the built-in preset ID selected by current
configuration. They cannot supply a URL, destination, or digest. A missing
explicitly configured `model_path` has configured-path provenance and does not
offer Download. The managed-download integrity contract is:

1. Verify that an existing cache entry is a regular file and its SHA-256 matches the built-in catalog; reuse it if valid.
2. Check remote metadata before transfer and count every streamed chunk, so missing or misleading `Content-Length` headers cannot bypass the preset's hard byte limit.
3. Compute SHA-256 after download; remove staging and fail on mismatch.
4. Write staging on the same filesystem and atomically persist it as the final GGUF; validate it again on later startups.

Downloading remains optional: a user can leave setup without a
network request. A corrupt or mismatched cache entry is not loaded, and checksum
failure never falls back to an unverified file. The current catalog uses
`revision = "main"` while pinning acceptable content by SHA-256. If upstream
replaces a file, the catalog digest must be reviewed and updated in code.

After installation, the in-process `agentd` epoch supervisor builds a
replacement Runtime. The old control plane remains online while it builds. The
supervisor starts the new endpoint and atomically publishes its descriptor
before retiring the old endpoint. Address, token, and server UUID rotate while
scope, PID, and the lifetime lock remain stable. Clients reconnect and
resynchronize from a Snapshot. A build or transition failure keeps the old
control plane available and publishes a retryable failure instead of exposing
a partially initialized Runtime.

## Native Windows prerequisites

A Windows CPU build needs:

- Visual Studio Build Tools with the MSVC C++ toolchain and Windows SDK;
- CMake and Ninja;
- LLVM/Clang with `libclang.dll` discoverable by the bindings generator; set `LIBCLANG_PATH` when needed.

The repository provides `./scripts/build-windows.ps1`. It discovers Visual Studio and LLVM and sets `LIBCLANG_PATH` plus the Ninja CMake generator only for the current process; it does not write global `CC` or `CXX`. The wrapper detects interrupted `llama-cpp-sys-2` build directories that contain `CMakeCache.txt` but no generated Ninja or Visual Studio project, then reruns CMake generation in place from the locked dependency's trusted source. By default it neither cleans nor recreates the shared `target` directory. Use that script for the default workspace build, including the sibling `agent` and `agentd` executables. Enable the `cuda` or `vulkan` feature for those backends and also install the matching SDK, driver, and toolchain prerequisites. `gpu_layers = 0` still selects the CPU path.

## Typed task routing

Routing does not ask a control model for a free-form plan. `TaskRouter` accepts a typed `StepContext` and returns `Complexity::{Low, Medium, High}`. The context also carries tool/JSON capability requirements, previous-step count, escalation state, data classification, and any provider/model explicitly selected by the user. The Scheduler then uses those fields to filter capability, privacy, and cost constraints and choose a Control, Worker, or Frontier tier.

The daemon-owned Qwen3 model can also serve as `ModelTaskRouter`. It receives a request with `temperature = 0`, at most 8 tokens, and thinking disabled, and may return only `LOW`, `MEDIUM`, or `HIGH`. If inference fails or the output is invalid, the Scheduler logs a warning and uses its deterministic heuristic classifier, keeping the router from becoming a single point of failure. “Typed” here means a Rust interface and a finite enum result, not an arbitrary JSON-schema decision API.

The current llama.cpp Rust API does not expose chat-template keyword arguments, so `thinking = false` appends Qwen3's official `/no_think` directive to the last user message before template rendering. Tool use follows a bounded `<tool_call>{...}</tool_call>` text protocol. The Provider accepts only one declared tool with object arguments that pass its JSON Schema, then converts it into a typed `ModelEvent::ToolCall`; ordinary answers remain text events.

## Why Laya is not yet a standard llama.cpp provider

Laya targets low-latency typed decisions such as choice, score, and noul rather than standard autoregressive chat completion. The current community GGUF conversion contains only the ModernBERT backbone; its decision head and configuration remain in a separate `safetensors` file and must execute outside llama.cpp. Lyonbrown's generic `LlamaCppProvider`, by contrast, expects a GGUF with usable `tokenizer.chat_template` metadata and produces a stream of text tokens.

Loading Laya's backbone in llama.cpp therefore does not satisfy Lyonbrown's `ModelProvider` contract. Laya is a better candidate for a dedicated `TaskRouter`/typed-decision adapter with separate validation for the head, tokenizer, schema, calibration, and backend parity. Until that exists, it is not presented as a standard llama.cpp provider. See [laya.cpp](https://github.com/lkarlslund/laya.cpp) and the [community GGUF architecture notes](https://huggingface.co/fr0stbit3/laya-gguf).
