# Configuration Reference

Lyonbrown uses `config-rs` to layer TOML, YAML, JSON, JSON5, RON, and INI. Precedence from lowest to highest is: built-in defaults, `~/.agent/config.toml`, workspace `agent.toml`, a `--config` file, `LYONBROWN_` environment variables, and CLI flags. Nested environment keys use double underscores, for example `LYONBROWN_MODEL__PROVIDER=ollama`. Workspace profile, model routing, provider endpoints, remote credentials, approval, and storage settings are rejected by default; pass `--trust-workspace-config` after reviewing the workspace configuration.

## Ollama

```toml
[model]
provider = "ollama"
model = "qwen3.8-27b:q3-claude"
max_tokens = 2048

[ollama]
endpoint = "http://127.0.0.1:11434"
```

Run `agent ask "hello"`, `agent chat`, or `agent .` inside a workspace to open the TUI. The TUI drives the Kernel through the shared `agent-app` command, snapshot, and approval contracts; the headless CLI retains a narrow direct Kernel entry point.

## Remote models and fallback

Configure an API-key reference, never a plaintext key. `env:` reads a process environment variable; `file:` reads `~/.agent/secrets/<name>`. The provider resolves the reference when it sends a request.

```toml
[model]
provider = "ollama"

[anthropic]
enabled = true
endpoint = "https://api.anthropic.com"
api_key_ref = "env:ANTHROPIC_API_KEY"

[openai]
enabled = true
endpoint = "https://api.openai.com/v1"
api_key_ref = "env:OPENAI_API_KEY"
```

Remote providers are disabled by default. Set `model.provider = "anthropic"` or `"openai"` to select one directly. Alternatively, keep a local provider selected and enable a remote provider: if a local Worker encounters a timeout or HTTP 5xx while opening the stream or before its first output, the Kernel can retry once on a remote Frontier model. Automatic fallback applies only to Public context; tool results and Confidential/Secret context are not sent remotely through fallback. Explicitly selecting a remote provider remains the user's data-boundary decision. HTTP 401/429 and failures after model output do not trigger fallback.

A repository's `agent.toml` can select profiles, providers, and service endpoints. The CLI rejects workspace profile, model, approval-policy, and storage settings by default; pass `--trust-workspace-config` to load them, and only when you trust that repository configuration.

## Tool approval

```toml
[approval]
approve_all = false
deny_all = false
timeout_seconds = 300
```

Interactive approvals deny after 300 seconds by default. Set `approval.timeout_seconds` to another positive value when needed. Client disconnects, mismatched response IDs, and timeouts all fail closed. The TUI defaults to Deny once, and `Enter` activates that safe action.

## MCP tools

```toml
[mcp.servers.files]
command = "C:/path/to/mcp-server.exe"
args = ["--stdio"]
```

The CLI connects to configured stdio servers at startup and registers tools as `mcp.files.<tool>`. MCP tools have Execute risk by default and use the common approval, 60-second timeout, and output-redaction path. MCP commands in workspace `agent.toml` are rejected by default; after trusting the workspace, use `agent --trust-workspace-mcp chat` (or the relevant subcommand). Model settings and MCP commands use separate trust flags. Global and explicit `--config` MCP configuration is treated as user-trusted. Prefer an explicit executable path.

## Session storage and resume

SQLite WAL storage is enabled by default at `~/.agent/agent.db`. Use `agent sessions` to find a session ID, then `agent resume <session-id>` to continue it. To disable storage or choose another path:

```toml
[storage]
enabled = true
path = "./.agent/session.db"
```

Relative storage paths resolve from the workspace. `LYONBROWN_STORAGE__ENABLED=false` and `LYONBROWN_STORAGE__PATH` are the environment equivalents. Event writes return success only after database commit. On resume, completed tool results are retained, while started calls without a recorded result are marked as having an unknown outcome and are not automatically replayed. In the TUI, the first Ctrl-C cancels the current turn and the second forces it to stop. Escape does not quit; use Ctrl-D or `/quit` for an explicit exit.

## Embedded llama.cpp

The llama.cpp provider is the default provider and a default `agent-cli` feature. The Agent loads GGUF directly in process, with no HTTP daemon or inference subprocess. The `cuda` and `vulkan` features are optional; a minimal `--no-default-features` build excludes it.

```toml
[model]
provider = "llamacpp"
model = "qwen-local"
max_tokens = 1024
thinking = false

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

Relative model paths are resolved from the workspace. `LYONBROWN_LLAMACPP__MODEL_PATH` is the equivalent environment setting. `model.thinking` defaults to `false`; for Qwen3 templates the embedded provider applies the official `/no_think` control before rendering. When `model_path` is omitted, Lyonbrown selects, downloads, and verifies a Qwen3 GGUF based on currently available RAM; see [Local Models and Task Routing](local-models-routing.md). Native Windows builds require MSVC, the Windows SDK, CMake/Ninja, and an LLVM `libclang` discoverable by the bindings generator; set `LIBCLANG_PATH` when necessary.

```powershell
cargo run -p agent-cli -- --config agent.toml ask "Introduce Rust in one sentence"
```

Selecting `llamacpp` without the feature produces an explicit error instead of falling back to an external service. For real-model acceptance, set `LYONBROWN_GGUF_MODEL` and run the provider's ignored live smoke test.
