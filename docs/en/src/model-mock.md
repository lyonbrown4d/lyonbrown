# Synthetic model service

`apps/model-mock` is a development/test application using Wiremock, Axum and existing protocol fixtures. It listens only on loopback, forwards to no real provider, starts no Ollama/llama.cpp/mistral.rs runtime and loads no weights. Shared `MockInferenceBackend` supports the in-process local inference boundary.

## Startup and integration

```sh
cargo run --locked -p model-mock -- --scenario apps/model-mock/examples/development.toml --listen 127.0.0.1:8100
```

Additional scenario files are `apps/model-mock/examples/decision.toml` and `apps/model-mock/examples/ollama.toml`. Use `--data-dir /absolute/path/mock-data` for logs. Configuration supports TOML/JSON, adjacent `.env`, process environment and CLI through a maintained configuration library; environment names use `LYONBROWN_MODEL_MOCK_`.

Agent development configuration can use the OpenAI-compatible endpoint:

```toml
provider = "openai-completions"
model = "mock-chat"
api_base = "http://127.0.0.1:8100/v1"
browser_tools = false
computer_tools = false
```

Set a synthetic `OPENAI_API_KEY=synthetic-model-mock-key` in adjacent `.env`, then run `cargo dev tui --config /absolute/path/config.toml` or `cargo dev desktop --config /absolute/path/config.toml`. Mocking the model does not relax workspace, tool or approval controls.

## Protocols and scenarios

| Endpoint | Response |
| --- | --- |
| `/v1/chat/completions` | Chat Completions JSON / SSE with function-tool results |
| `/v1/responses` | Responses JSON / named SSE |
| `/v1/messages` | Anthropic Messages JSON / named SSE |
| `/v1/models` | Synthetic model catalog |
| `/api/chat` | Native Ollama JSON / NDJSON with tool calls and terminal done metadata |
| `/api/generate` | Native Ollama generation JSON / NDJSON |
| `/api/tags`, `/api/show`, `/api/version` | Synthetic local-model and service metadata |
| `/health` | Health and request counts without request bodies or credentials |

Scripts match model and protocol, consume replies in order and can repeat the final reply. Cases cover text, decision JSON, tools, error statuses, delay, stalled streams, malformed/truncated output and missing terminal completion. Request size, concurrency, script length and delays are bounded.

`mock-chat` serves ordinary integration; `mock-tools` exercises tool round trips. Decision scenarios distinguish reusable typed judgments from Agent-specific tool plans containing task kind, tools, execution mode and confidence. Low confidence and malformed JSON test Unknown/fallback/tool-free behavior and never authorize actions.

`ollama.toml` provides these ready-to-run scenarios. Omitting `stream` keeps Ollama's streaming default.

| Model ID | Native endpoint and behavior |
| --- | --- |
| `mock-local:latest` | Chat / Generate JSON and NDJSON, plus OpenAI-compatible Chat Completions |
| `mock-tools:latest` | Chat emits a function name and typed arguments, then text |
| `mock-delay:latest` | Chat delays the first response for timeout and cancellation |
| `mock-truncated:latest` | Chat closes after `done = false`, without terminal metadata |
| `mock-missing-terminal:latest` | Chat emits partial output then waits, exercising cancellation and capacity release |
| `mock-malformed:latest` | Generate returns malformed JSON |
| `mock-error:latest` | Generate returns a synthetic service error |
| Unconfigured model ID | Returns 404 without substituting another model |

Native Ollama shapes and streaming are tested through `ollama-rs`. Agent HTTP integration uses the OpenAI-compatible `/v1` endpoint with `provider = "openai-completions"`.

## In-process local inference mock

`agent-test-support::MockInferenceBackend` implements the actual `InferenceBackend` port and registers in `ModelRegistry` for Agent or `decision-model` consumers. Scripts support text/thinking/tool chunks, delay, errors, finish reasons, cancellation and backpressure. Counters distinguish requests, completions, cancellations and failures. No HTTP process is required.

Local integration tests traverse Registry, Harness and real SQLite, asserting final Run state, effective tools and route events. Remote cases traverse actual protocol adapters. Mock results validate integration, protocols and state rather than model quality, inference performance, context limits or native SDK correctness.

```sh
cargo test --locked -p model-mock -p agent-test-support
cargo test --locked -p agentd --test local_mock
```
