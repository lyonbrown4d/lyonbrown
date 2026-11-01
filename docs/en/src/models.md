# Models and execution decisions

## Terminal catalog and installation

`/models` reads agentd's authenticated unified catalog of builtin API models, configured models and installed Hugging Face artifacts. `configurable`, `installed_unverified` and `ready` distinguish missing credentials, downloaded files and an available execution binding. API credential presence does not establish successful inference. Select using `/models select MODEL EXECUTION`. agentd checks session ownership and epoch and rejects changes while runs are active or uncertain. SQLite stores the selection and copies an immutable execution binding when accepting a Run.

```text
/models search "SmolLM2"
/models inspect OWNER/REPO REVISION
/models plan OWNER/REPO REVISION FILE llama_cpp
/models install OWNER/REPO REVISION FILE llama_cpp PLAN_ID
/models status PLAN_ID
/models cancel PLAN_ID
/models installed
/models load PLAN_ID
/models select PLAN_ID local:PLAN_ID
/models unload PLAN_ID
/models remove PLAN_ID
```

Runtime is `llama_cpp` or `mistral_rs`. Installation requires confirmation of a plan ID fixing the commit and file set. GGUF selects one artifact; safetensors plans include configuration, tokenizer and shards without executing repository code. The official Rust `hf-hub` client downloads files; agentd verifies sizes and SHA256 before activation. Cancellation preserves downloads. Pinned partial files with an expected hash support Range continuation; other partial files restart. Full SHA256 verification still applies, and ignored Range requests or corrupted contents cannot activate. Removal quarantines artifacts for administrator recovery.

SQLite stores intent, events and Outbox before downloading. `status` polls persistent progress. Restart converts incomplete jobs to `uncertain`; inspect before explicitly resuming. Downloads finish as `installed_unverified`; `load` invokes the actual inference backend before selection is allowed. Build agentd with `local-llama` or `local-mistral`. agentd owns `HF_ENDPOINT` / `--hub-endpoint`, `HF_TOKEN`, downloads and loading; the TUI renders replies.

Loading currently uses CPU settings. Automatic hardware recommendations, idle eviction, update rollback and automatic reload after restart are unavailable. File format alone does not establish architecture compatibility. Hub replies currently render JSON, with progress queried through `status`.

Local tool calling defaults to disabled. Set `supports_tools = true` on a configured Profile, or start agentd with `--local-model-tools` before Hub `load`, to explicitly request mistral.rs tool templates and parsing. This operator assertion still requires acceptance for the particular model. The llama adapter rejects tool capability declarations. Capabilities do not grant tools or bypass approval; agentd's execution plan controls actual authority.

mistral.rs accepts a Profile `chat_template` or daemon `--local-model-chat-template`: an absolute UTF-8 `.jinja` file of 1 byte to 1 MiB, without symlinks. The llama adapter rejects this override. The adapter uses the public explicit-template API and the public `RequestLike` port for canonical `tool_calls`; see [mistral.rs #2528](https://github.com/EricLBuehler/mistral.rs/issues/2528).

Linux CPU acceptance confirmed actual file reading and returning the result across turns. The 0.5B model repeatedly added an invalid directory-listing field, so strict four-turn acceptance failed. The second turn reread the file and does not prove history-only recall. Larger-model inference acceptance was deferred; these results do not establish reliable use of every tool by every model.

Run actual CPU inference acceptance with `cargo test --profile native-acceptance -p agentd --features local-mistral --test model_native_tools -- --ignored --nocapture`. This profile optimizes Rust inference kernels without LTO; unoptimized debug builds are not suitable for assessing local response capability. The test explicitly downloads pinned weights and checks actual workspace tool events and multi-turn results. Mock success does not replace this acceptance.

The Agent embeds the reusable core by default, pins the configured main model and persists `model_route_decision` before the Agent-specific tool plan. See [Reusable decision engine](decision-engine.md) for library/backends and [AI Gateway](gateway.md) for independent serving.

Each run receives an execution plan from `agent-decision` before `agent-runtime` opens `rpi-harness`. A classifier returns advice. It executes no tools and cannot grant permissions.

## How the plan controls execution

Without `decision.model`, the daemon uses an explicit workspace rule: the general model policy, `read_file`, `list_files`, `search_files`, and revision-checked `write_file`, with MCP, browser and trusted-command tools selected by configuration and available runtime capabilities. Configured, actively paired delegation targets can enable distributed execution and `delegate_task`. Workspace path restrictions and per-operation mutation approval still apply. This rule comes from daemon policy; prompt text cannot supply authorization.

When configured, the classifier receives the user request as data. It must return one JSON object containing `task_kind`, `model_policy`, `allowed_tools`, `execution_mode`, `target_hint`, `risk_level` and `confidence`. Unknown fields, invalid enums, wrong types and confidence outside the allowed range are rejected.

```mermaid
flowchart TB
    Input[Accepted user request] --> Select{decision.model configured?}
    Select -->|No| Rule[Explicit workspace rule]
    Select -->|Yes| Classify[Tool-free classification request]
    Classify --> Validate[Validate JSON and confidence]
    Validate -->|Valid| Clamp[Restrict advice to daemon authority and capabilities]
    Validate -->|Invalid or timed out| Fallback[Local general model / no tools]
    Rule --> Persist[Persist execution_plan event]
    Clamp --> Persist
    Fallback --> Persist
    Persist --> Filter[Filter actual Harness tool set]
    Filter --> Run[rpi-harness executes]
    Run --> Approval[Mutating tools still require human approval]
```

The tool set is the intersection of the classifier's selection, authorized tools and available capabilities. Model policies, distributed execution and target nodes are independently constrained by allowlists. Delegation targets must appear in both operator configuration and active pairing records. Local plans do not expose delegation. Classifier output cannot widen tool, model or node authority. See [Delegating to paired nodes](tools.md#delegating-to-paired-nodes) for configuration and approval scope.

The effective plan is persisted as an `execution_plan` event before opening the Harness. It records tools, capabilities, execution mode, risk, confidence, source and `trace_id`. The daemon filters the actual Harness tools against this plan, restricting both model-visible tools and execution. Approval consumption checks the run, tool, full arguments and expiry; a classification never replaces human approval.

Unavailable classifiers, timeout, invalid output, insufficient confidence or missing required capabilities fall back to local execution with the configured main model and no tools or target node. Cancellation during classification persists a cancelled run without calling the main model.

## Remote protocols

| `provider` | Protocol | Credential |
| --- | --- | --- |
| `openai`, `openai-responses` | OpenAI Responses | `OPENAI_API_KEY` |
| `openai-completions` | OpenAI Chat Completions | `OPENAI_API_KEY` |
| `anthropic` | Anthropic Messages | `ANTHROPIC_API_KEY` |

Requests use the actual provider adapter. `openai` aliases Responses; verify which API mode a compatible service supports. `supports_images` is explicit and defaults to false. A model name does not establish image support. Classification requests disable images and tools.

## Configure a classifier

Use `config.toml` in the data directory. Put credentials in its adjacent `.env` or the process environment:

```toml
provider = "openai"
model = "gpt-4.1"
supports_images = false

[decision]
model = "classifier-model-id"
provider = "openai-completions"
api_base = "https://models.example.com/v1"
timeout_ms = 3000
minimum_confidence = 0.75
```

The classifier model ID and domain are examples. Replace them with the service's actual model and endpoint. Omit `[decision]` to use the workspace rule.

| Setting | Environment variable | Default and constraint |
| --- | --- | --- |
| `decision.model` | `LYONBROWN_DECISION_MODEL` | Unset; must be nonempty when set |
| `decision.provider` | `LYONBROWN_DECISION_PROVIDER` | Inherits the main provider |
| `decision.api_base` | `LYONBROWN_DECISION_API_BASE` | Inherits the main endpoint for the same provider; otherwise uses the selected provider's default |
| `decision.timeout_ms` | `LYONBROWN_DECISION_TIMEOUT_MS` | `3000`; accepts `1..=60000` milliseconds |
| `decision.minimum_confidence` | `LYONBROWN_DECISION_MINIMUM_CONFIDENCE` | `0.75`; accepts `0..=1` |

## Local inference boundary

`agent-inference` adapts `llama-cpp-2` and `mistral.rs`. Both generate model output; the same `rpi-harness` retains session and loop ownership. The daemon loads configured local files into its registry before becoming ready. Load failure prevents startup without downloading models or falling back to a cloud provider.

```toml
provider = "local"
model = "local-main"

[[local_models]]
id = "local-main"
backend = "llama"
format = "gguf"
path = "/absolute/path/to/chat-model.gguf"
context_window = 4096
max_output_tokens = 512
parallel_requests = 1
max_model_bytes = 8589934592
```

Replace the example path with an existing local model. Windows requires a native absolute path. Build `agentd` with `local-llama` for `llama`, or `local-mistral` for `mistral`. Llama reads GGUF; Mistral accepts `gguf` or `directory`. The selected engine must support the actual model architecture and quantization; a matching extension alone does not establish compatibility.

Profiles explicitly select CPU execution without implicit GPU allocation. `max_model_bytes` bounds input file size, rather than runtime RAM. Context, output tokens and concurrency have separate limits. Each run persists actual metadata in `model_selected`. Text-only backends do not advertise image or tool support; the daemon removes unsupported tools before persisting the effective plan and opening the Harness.

To select a local classifier independently, set `decision.provider = "local"` and point `decision.model` to a configured profile ID. `LocalClassifier` accepts structured output without tool calls and applies the same JSON, confidence, deadline and cancellation checks. Invalid output takes the conservative fallback. A local main model needs no cloud credentials.

See [Development and verification](development.md) for execution and acceptance commands, and [Connect MCP tools](mcp.md) for MCP configuration.

Source references: [decision validation](https://github.com/lyonbrown4d/lyonbrown/blob/main/crates/agent-decision/src/engine.rs), [daemon composition](https://github.com/lyonbrown4d/lyonbrown/blob/main/apps/agentd/src/decision.rs), [execution and persistence](https://github.com/lyonbrown4d/lyonbrown/blob/main/apps/agentd/src/execution.rs), [configuration](https://github.com/lyonbrown4d/lyonbrown/blob/main/crates/agent-config/src/settings.rs).
