# AI Gateway

Gateway is embedded in agentd. It routes model requests, enforces access restrictions and records request metadata; it does not run an Agent Loop. There is no standalone Gateway process or Web administration interface. The Agent retains responsibility for Sessions, tools, permissions and approvals.

The embedded Gateway currently handles OpenAI Chat Completions. Responses, Gemini and Anthropic use their native adapters, without an automatic Gateway fallback. Native local inference also uses its own backend and does not enter the Gateway model pool. The optional HTTP interface accepts Chat Completions, not Responses or Anthropic; images are restricted to the standard user inline parts described below, and audio is not supported.

## Configuration

Gateway uses agentd configuration. `http_listen` defaults to unset, so the HTTP listener is off. When enabled, it accepts only a nonzero loopback socket; external TLS and remote listening are not implemented. Set `http_token_env` to the name of a dedicated environment variable. Its value must be 32–512 printable ASCII characters and differ from both the daemon credential and the internal Gateway credential. All HTTP routes, including health checks and OpenAPI JSON, require this Bearer token.

The following example defines two Chat model IDs, two upstream accounts and a group. Replace the example HTTPS addresses, upstream model names and environment variable values with those for the deployment. Environment variables may be supplied by the process or the configuration file's adjacent `.env`; do not put credential values in TOML.

```toml
[gateway]
http_listen = "127.0.0.1:8099"
http_token_env = "GATEWAY_HTTP_TOKEN"

[[gateway.models]]
id = "chat-a"
upstream_model = "model-a"
api_base = "https://account-a.example.com/v1"
api_key_env = "ACCOUNT_A_KEY"
context_tokens = 128000
capabilities = ["text", "tools"]

[[gateway.models]]
id = "chat-b"
upstream_model = "model-b"
api_base = "https://account-b.example.com/v1"
api_key_env = "ACCOUNT_B_KEY"
context_tokens = 128000
capabilities = ["text", "tools"]

[[gateway.endpoints]]
id = "account-a"
provider_id = "provider-a"
api_base = "https://account-a.example.com/v1"
api_key_env = "ACCOUNT_A_KEY"
model_ids = ["chat-a"]
priority = 0

[[gateway.endpoints]]
id = "account-b"
provider_id = "provider-b"
api_base = "https://account-b.example.com/v1"
api_key_env = "ACCOUNT_B_KEY"
model_ids = ["chat-b"]
priority = 0

[[gateway.groups]]
id = "chat-group"
models = ["chat-a", "chat-b"]
```

An endpoint must list configured model IDs. Remote upstreams require HTTPS; HTTP upstreams require a literal loopback IP. Model and group selection remains subject to server-side authorization and capability checks. A request cannot expand these restrictions. The HTTP model routes are `GET /v1/models` and `POST /v1/chat/completions`; health routes and `/api-doc/openapi.json` are also available when the listener is enabled and authenticated.

## Privacy and persistence

The shared privacy service processes user prompts and tool output before model use and persistence. It replaces detected sensitive text with scoped references. Encrypted mappings are stored in the session database using the host master key; writes check the Session owner epoch. Mapping plaintext is limited to 16 KiB. A protection, encryption or persistence failure rejects the operation rather than passing the original content through.

Stored credentials are registered at startup, and uploaded or rotated values are registered before use. Sessions and the HTTP entry point share recognition. Dynamic known values are limited to 4096 entries and 1 MiB total. Provider credential replacement or removal retains deduplicated encrypted recognition history, up to 1024 entries, so old values remain protected after restart. This history is not used for upstream authentication; changes are rejected when capacity is exhausted.

Runtime response text is buffered until validation completes, so it is not shown incrementally. A tool reference is denied by default until the actual resolved target is authorized. Text detection does not protect images in the native runtime. Embedding requests containing detected sensitive text fail closed until a stable projection is available; no real local privacy model is loaded.

The Gateway uses Rig's public HTTP transport and SSE framing. It limits requests, responses, concurrency and time, and checks terminal state. An interrupted request or missing terminal marker does not prove that an upstream did not execute; it must not be blindly replayed. Audit records contain metadata rather than prompts or response bodies.

Runtime and daemon integration tests use local synthetic upstreams without paid model calls. They cover model selection, authentication of the optional HTTP listener, and privacy mapping recovery after restart. They do not establish real-model, native GUI, three-platform or public-network acceptance.


## Inline images and model capabilities

Chat images are accepted only as standard `image_url` parts in user messages, with inline base64 data URIs for PNG, JPEG, WebP or GIF. Gateway reuses shared image validation for encoding, actual MIME type, pixels and aggregate limits. It does not download remote images or accept audio or arbitrary opaque content. Only the exact validated image field retains its bytes; text still passes through shared sensitive-content protection. Image contents are not automatically redacted and require user review before sending.

Image requests require an explicit `vision` model capability. Direct selection and every candidate in a model group enforce it, so a text-only model cannot receive an image through fallback. Automatic single-model configuration and caching use explicit `supports_images`, never a model-name guess.

Images remain subject to Gateway `max_body_bytes`: 256 KiB by default, configurable up to 16 MiB. This can be lower than daemon attachment limits; an attachment within its own limit is not necessarily within the current Gateway request limit.
