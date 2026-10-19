# 合成模型服务

`apps/model-mock` 是开发与测试应用，使用 Wiremock、Axum 和现有协议 fixture 提供合成响应。它只监听回环地址，不转发到真实 Provider，不启动 Ollama、llama.cpp 或 mistral.rs，也不加载权重。共享的 `MockInferenceBackend` 则用于进程内本地推理接口测试。

## 启动与接入

```sh
cargo run --locked -p model-mock -- --scenario apps/model-mock/examples/development.toml --listen 127.0.0.1:8100
```

独立场景文件还有 `apps/model-mock/examples/decision.toml` 和 `apps/model-mock/examples/ollama.toml`。用 `--data-dir /absolute/path/mock-data` 指定日志位置。配置使用成熟配置库，支持 TOML/JSON、相邻 `.env`、进程环境和 CLI；环境变量前缀为 `LYONBROWN_MODEL_MOCK_`。

Agent 的开发配置可指向 OpenAI 兼容接口：

```toml
provider = "openai-completions"
model = "mock-chat"
api_base = "http://127.0.0.1:8100/v1"
browser_tools = false
computer_tools = false
```

在相邻 `.env` 设置合成值 `OPENAI_API_KEY=synthetic-model-mock-key`，然后用 `cargo dev tui --config /absolute/path/config.toml` 或 `cargo dev desktop --config /absolute/path/config.toml` 启动。模型 Mock 不会放宽 Agent 的工作区、工具或审批限制。

## 协议与场景

| 接口 | 响应形式 |
| --- | --- |
| `/v1/chat/completions` | Chat Completions JSON / SSE，函数工具与结果 |
| `/v1/responses` | Responses JSON / 命名 SSE 事件 |
| `/v1/messages` | Anthropic Messages JSON / 命名 SSE 事件 |
| `/v1/models` | 合成模型目录 |
| `/api/chat` | Ollama 原生聊天 JSON / NDJSON，工具调用与 done 终态 |
| `/api/generate` | Ollama 原生生成 JSON / NDJSON |
| `/api/tags`、`/api/show`、`/api/version` | 合成的本地模型与服务信息 |
| `/health` | 健康状态与请求计数，不返回原始请求或凭据 |

场景按模型与协议匹配，响应按脚本顺序消费，可选择重复最后一条。支持文本、决策 JSON、工具调用、错误状态、延迟、停滞、损坏响应、截断和缺少终态。请求大小、并发、脚本长度及延迟均有限制。

`mock-chat` 用于普通接入，`mock-tools` 用于工具往返。决策场景将通用结构化判断与 Agent 专属工具计划分开：前者返回 Operation 对应的类型化输出，后者返回 `task_kind`、工具集合、执行模式与置信度。低置信度和错误 JSON 用于验证 Unknown、降级与无工具行为，不能作为允许操作的依据。

`ollama.toml` 提供以下可直接启动的场景；不传 `stream` 时沿用 Ollama 的流式默认值。

| 模型 ID | 原生接口与行为 |
| --- | --- |
| `mock-local:latest` | Chat / Generate JSON、NDJSON；另有 OpenAI 兼容 Chat Completions |
| `mock-tools:latest` | Chat 返回函数名和类型化参数，随后返回文本 |
| `mock-delay:latest` | Chat 延迟首个响应，用于超时与取消 |
| `mock-truncated:latest` | Chat 在 `done = false` 后关闭，缺少终态 |
| `mock-missing-terminal:latest` | Chat 输出部分内容后持续等待，用于取消和容量释放 |
| `mock-malformed:latest` | Generate 返回损坏的 JSON |
| `mock-error:latest` | Generate 返回合成服务错误 |
| 未配置的模型 ID | 返回 404，不替换成其他模型 |

Ollama 原生接口用 `ollama-rs` 客户端验证响应形状和流式解析。Agent 的 HTTP 接入使用其 OpenAI 兼容 `/v1` 接口，Provider 配置为 `openai-completions`。

## 本地推理接口 Mock

`agent-test-support::MockInferenceBackend` 实现真实的 `InferenceBackend` 端口，可注册到 `ModelRegistry`，供 Agent 或 `decision-model` 使用。脚本支持文本/Thinking/ToolCall chunk、延迟、错误、停止原因、取消和背压，计数区分请求、完成、取消和失败。进程内 Mock 不需要启动 HTTP 服务。

本地集成测试经过 Registry、Harness 和真实 SQLite，检查最终 Run、有效工具计划与模型路由事件；远程测试经过真实协议适配器检查请求与响应。Mock 证明接入、协议与状态行为，不证明模型质量、推理性能、上下文上限或原生 SDK 正确性。

```sh
cargo test --locked -p model-mock -p agent-test-support
cargo test --locked -p agentd --test local_mock
```
