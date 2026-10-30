# AI Gateway

`ai-gateway` is an independent Rust service, listening on `127.0.0.1:8099` by default. Typed FluxDI modules and Axum request scopes assemble the decision core, SQLite persistence, forwarding and Leptos administration UI. It needs no agentd. An Agent using the gateway still owns its Harness, Session, tools and approvals.

## Startup and configuration

```sh
cargo gateway --config /absolute/path/gateway.toml --data-dir /absolute/path/gateway-data
```

Run from the repository root. `cargo gateway` invokes the independent `gateway-dev` launcher, builds the server and browser assets, then starts Gateway without agentd. First use prepares the official `wasm32-unknown-unknown` target for the current toolchain through rustup and installs fixed cargo-leptos 0.3.11 under the repository's `.local/tools`; no manual global installation is required. Existing tools must pass version verification. See [Development and verification](development.md#gateway-full-stack-development) for options and build flow.

Configuration layers defaults, TOML, adjacent `.env`, process environment and CLI. `--listen` overrides the address last; only nonzero loopback listeners are accepted. Environment names include `LYONBROWN_GATEWAY_CONFIG`, `LYONBROWN_GATEWAY_DATA_DIR`, `LYONBROWN_GATEWAY_LISTEN`, `LYONBROWN_GATEWAY_WEB_ROOT`, `LYONBROWN_GATEWAY_TOKEN`, `LYONBROWN_GATEWAY_ADMIN_USERNAME` and `LYONBROWN_GATEWAY_ADMIN_PASSWORD`.

```toml
listen = "127.0.0.1:8099"

[gateway]

[[gateway.models]]
id = "local-chat"
upstream_model = "deployed-model-id"
api_base = "http://127.0.0.1:8080/v1"
api_key_env = "LOCAL_MODEL_API_KEY"
tier = "balanced"
local_inference = true
capabilities = ["text", "tools"]
context_tokens = 4096
```

Replace the endpoint with a deployed service. `api_key_env` selects an environment or dotenv credential instead of putting it in the model configuration.

The optional `gateway.endpoints` list maps public model IDs to one or more upstream endpoints. A model with no endpoint mapping continues to use `api_base` and `api_key_env` from `gateway.models`; an empty endpoint list leaves all models on this legacy configuration. Once an endpoint maps a model ID, Gateway selects only from that model's mapped endpoints. If they are all disabled, cooling down or unavailable, it does not fall back to the legacy model URL.

```toml
[[gateway.endpoints]]
id = "local-primary"
provider_id = "local-provider"
api_base = "http://127.0.0.1:8080/v1"
api_key_env = "LOCAL_MODEL_API_KEY"
enabled = true
priority = 10
max_concurrent = 8
model_ids = ["local-chat"]
```

`id` and `provider_id` are configuration identifiers. `api_base` is the Chat Completions API base URL; `api_key_env` reads the credential from the environment or dotenv. `model_ids` must list the public model IDs served by this endpoint. Lower `priority` values are tried first, with `id` breaking ties. `enabled` defaults to true, `priority` to `0`, and `max_concurrent` to `1`. The application configuration accepts `api_key_env`, not an inline endpoint secret. Remote endpoints require HTTPS; HTTP is allowed only for a literal loopback IP.

The administration UI uses username/password sign-in. First startup initializes the single super administrator from environment variables or adjacent `.env`. There is no built-in account or default password.

| Variable | Requirement |
| --- | --- |
| `LYONBROWN_GATEWAY_ADMIN_USERNAME` | 1–64 ASCII characters: letters, digits, `_`, `.`, `@` or `-`; case-sensitive |
| `LYONBROWN_GATEWAY_ADMIN_PASSWORD` | 15–128 password bytes; distinct from the API Key |
| `LYONBROWN_GATEWAY_TOKEN` | Independently generated 32–512 printable random bytes; Bearer authentication for model APIs only |

Administrator credentials cannot be placed in TOML. SQLite stores the username, super-administrator role and salted Argon2id password hash. It stores no plaintext password. Subsequent startup validates the same credentials; different credentials fail startup without overwriting the account or resetting its password. API Keys cannot sign in to the UI, and the administrator password cannot authenticate model API requests. The super administrator has access to the model catalog, routing policies and request records.

Gateway stores upstream credentials, API authentication keys, admin-session secrets and newly issued API tokens in `secrecy` 0.10.3 secret containers; password workers also use these containers. Access is explicit, `Debug` redacts their contents, and dropping a container zeroizes its owned memory. This does not guarantee erasure of every copy created by configuration parsing, environment variables, HTTP handling or temporary `String` values.

Keep Gateway TOML and its credential `.env` in a separate configuration directory. The launcher does not put administrator passwords or model credentials in build arguments or explicitly forward them to build children. However, cargo-leptos itself reads workspace or parent-directory `.env` files; environment filtering does not guarantee that build tools cannot access credentials.

`local_inference = true` is a trusted operator assertion about actual local inference and requires a literal loopback address. A loopback proxy to cloud inference must not be declared local. Remote upstreams require HTTPS; clients cannot choose arbitrary upstream URLs.

When the request or server policy requires `local_only`, the model must be marked as local by trusted configuration and the selected endpoint address must also be a literal loopback IP. A client can narrow scope with `routing.local_only`, but cannot grant a model local-inference status. A loopback endpoint alone does not prove that the model runs locally.

## Build assets and distribution

The launcher uses the workspace's `gateway` cargo-leptos project. Assets default to `.local/gateway-web/site/ui/pkg`. `--build-only` builds without starting a server; `--release` selects release builds. The launcher reads the server executable from Cargo's build results, preserves settings such as `CARGO_TARGET_DIR` and passes the actual site root to Gateway.

Distribute the `ai-gateway` executable together with its generated static assets. `LYONBROWN_GATEWAY_WEB_ROOT` must be an absolute site-root path containing `ui/pkg`; it defaults to a `web` directory beside the executable. Standard builds generate `ui/pkg/gateway_web.js` and `ui/pkg/gateway_web.wasm`, served under `/ui/pkg/`. Running only `cargo run -p ai-gateway` does not generate browser assets. Missing or empty assets produce an explicit error before database bootstrap. Use the standard full-stack build and distribute the binary and assets from the same build together.

## HTTP and Swagger

Gateway uses `utoipa-axum` 0.3.0 `routes!` and `OpenApiRouter` to register Axum routes and generate OpenAPI from the same handlers. Model endpoints and public health checks are assembled separately; Bearer authentication middleware applies only to the protected model endpoints.

| Method and path | Purpose and access |
| --- | --- |
| `GET /health/live` | Process liveness |
| `GET /health/ready` | Validated configuration and admission state; not an upstream health probe |
| `GET /v1/models` | Public model IDs allowed for the current API key; API Bearer key required |
| `POST /v1/chat/completions` | Text/function-tool Chat Completions forwarding; API Bearer key required |
| `GET /swagger-ui/` | Official read-only Swagger UI |
| `GET /api-doc/openapi.json` | Code-generated `utoipa` OpenAPI |
| `GET /ui/login` | Administration login |
| `GET /ui/keys` | API key management; super-administrator session required |

Swagger and the specification are accessible on the local listener. Business operations still require an API key through Authorize. The schema contains no live model catalog or credentials, and UI assets are embedded without CDN requests. The inbound protocol is Chat Completions; images, audio, Responses and Anthropic inbound requests are outside this endpoint's compatibility scope.

Gateway upstream transport reuses the public raw HTTP interfaces and `SseFramer` from [Rig-Core 0.45.0](https://docs.rs/rig-core/0.45.0/rig_core/), with [Rig-Reqwest 0.45.0](https://docs.rs/rig-reqwest/0.45.0/rig_reqwest/)'s `ReqwestMiddlewareClient`. Gateway retains supported request fields such as `n` and forwards JSON and SSE response bytes unchanged, including multiple choices, tool-call fragments and usage. Middleware immediately drops non-2xx response bodies and exposes only the same status with an empty body to Rig. Gateway retains byte limits, strict `[DONE]` checks, deadlines, cancellation and auditing. It uses no Rig Agent or Agent Loop; Agent and `model-provider` continue using rpi-ai's Responses, Chat Completions and Anthropic protocol adapters.

An explicit public model ID routes directly after validation; unknown or incapable models are not silently substituted. `router/auto` uses the [shared decision engine](decision-engine.md), followed by capability, context and locality revalidation. Request `routing.local_only` and `routing.allowed_tiers` can only narrow server policy and are removed before forwarding.

SSE tool fragments, usage and data bytes are retained while a maintained SSE parser checks terminal completion. Body/response sizes, concurrency, first-byte and total deadlines are bounded. Admission remains held until audit persistence finishes and the body is drained, dropped or reaches its total deadline; an early terminal marker cannot bypass capacity held by an unread body. Disconnect drops the upstream response; interrupted or unterminated streams remain uncertain without automatic retry. Errors never echo upstream bodies, prompts or credentials.

The first-byte budget starts when the request is accepted and includes automatic routing. Routing expiry returns `504 routing_timeout` without dispatching an upstream request.

An explicit upstream `429` cools that endpoint for 30 seconds for future selections in the current process. An upstream `5xx` cools it for 5 seconds. Cooldowns affect later requests; Gateway does not replay or switch the current request. The selected endpoint's concurrency permit, like Gateway's request permit, stays held until terminal auditing finishes and the response body is drained, dropped or reaches its total deadline.

## SQLite and administration

Diesel manages `gateway.sqlite` in the data directory with WAL, busy timeout and full synchronous writes. A single owner holds a database lock; startup reconciles abandoned Pending requests to Uncertain. Tables retain model snapshots, policy history, API key hashes and scopes, admin accounts, sessions and request metadata, with no prompt or completion body fields.

Database scripts are embedded with `diesel_migrations` 2.3.2 `embed_migrations!` and tracked by Diesel’s migration ledger. One SQLite immediate transaction bridges legacy `gateway_migrations` histories containing version 1 or versions 1 and 2, applies pending scripts and reconciles Pending requests. Already applied scripts are not replayed. Unknown versions, gaps, conflicting ledgers or existing application tables without a ledger fail startup and roll back the transaction.

A Pending intent is persisted before upstream dispatch. Succeeded means the upstream success terminal was received and validated, rather than proving client consumption. Database write ownership is independent of HTTP deadlines, so started intent and terminal writes still finish after the caller times out. Actual database failures can leave Pending for startup reconciliation to Uncertain; closing HTTP does not prove inference never happened.

### API keys and token budgets

Super administrators create and revoke API keys at `/ui/keys`. Creation requires a valid administrator session, the correct `Origin`, and a CSRF token bound to that session. A newly issued key is shown only once. The database stores its hash and management metadata, never the secret for later display. Each key can have a project label, allowed model IDs, a per-choice output cap, a cumulative token budget and an expiry from 1 to 31,536,000 seconds (the form defaults to 30 days). An empty model list means no model-ID restriction; leaving the output cap or budget blank leaves that limit unset.

`project_id` labels request ownership; it does not combine keys under a shared project budget. Budgets accumulate per key, count input plus output tokens, do not reset periodically, and are not monetary billing.

Before dispatch, Gateway writes the Pending audit record and reserves that key's budget in the same local SQLite immediate transaction. The conservative reservation is the validated request JSON's UTF-8 byte length plus the per-choice output limit multiplied by `n`. If the remaining budget cannot cover the reservation, Gateway returns `429` without sending the upstream request. When valid terminal usage is available, the known input-plus-output total replaces the reservation. Cached-input and reasoning counts are subsets of those totals; they are recorded for audit and are not counted again.

If the upstream omits usable usage, Gateway keeps the reservation rather than treating unknown usage as zero. Uncertain outcomes also keep the full reservation. On restart, unfinished Pending records become Uncertain and their reservations remain charged. Actual usage can exceed the reservation; Gateway records the actual total even if it exceeds the configured budget. Request records contain project and key identity, model, status, reservation and usage metadata, but no prompt or completion body.

The UI shares one Rust Leptos component tree. The server renders HTML first; generated JavaScript boots the browser WASM, which attaches events and reactive state to the existing page. There is no separate Node frontend stack. Pages cover the model catalog, routing policy, paginated request metadata and API key management, following system light/dark preferences.

Shared components provide layout, form fields, submission state and notices. Leptos Signals manage policy drafts, dirty state, immediate validation and reset; official `ServerAction` / `ActionForm` handle login, save and logout. Tables reuse the public `leptos-struct-table` `TableRow` derive and row-rendering interfaces. Page components own headings, semantic table markup and pagination, so SSR emits row content directly without requiring browser virtualization for the initial page.

`gateway-validation` uses [Garde 0.23.0](https://docs.rs/garde/0.23.0/garde/) derive macros and regex rules for basic `Credentials` and `PolicyDraft` validation shared by SSR, WASM, application configuration and storage entry points; authorization, CSRF, `PolicySnapshot` semantics and CAS remain server responsibilities.

Browser validation provides interaction feedback. Sensitive reads, password verification, the super-administrator role, session expiry, Origin and session-bound CSRF remain server checks. Expiring HttpOnly / SameSite=Strict cookies identify admin sessions. Policy writes and logout verify origin and CSRF. Policy publication validates the whole document and compares the previous revision; concurrent edits return a conflict. Upstream credentials and password hashes are absent from browser DTOs.

Cookie parsing and construction use `cookie` 0.18.2. The server examines all Cookie headers and rejects duplicate session or CSRF cookies and invalid values. Library parsing does not replace Origin, role, expiry or CSRF checks.

Login attempts share one process-local direct GCRA limiter from `governor` 0.10.4: 10 attempts per minute, burst capacity 10, with one opportunity replenished every 6 seconds rather than a fixed 60-second reset window. Client IPs and forwarded headers do not select separate quotas. Rejection includes a dynamic `Retry-After`, rounded up to seconds with a minimum of 1; enhanced forms receive 429, while native HTML forms redirect to the login page.

Deployment configuration owns the model catalog, recorded as immutable snapshots at startup. The UI does not edit upstream credentials. Gateway provides cumulative token budgets per API key, but no shared project budget or monetary billing. It also includes no provider-account management or public deployment. `Gateway::observations()` exposes process-local metrics: successful terminal requests with and without usage, and cumulative input, output, cached-input and reasoning tokens. These usage and token counters use saturating addition and update only after terminal audit persistence succeeds; missing usage is distinguishable from an actual zero. Process restart resets these in-memory metrics, while persisted request audit records remain. Shared rolling structured logs persist in the data directory.

## Validation

Gateway backend tests use local synthetic upstreams to cover JSON/SSE usage parsing and byte preservation, token-budget reservation and settlement, endpoint selection and cooldowns, authentication and key scopes; they do not call paid models. SSR tests and WASM hydrate compilation commands are in [Development and verification](development.md#reusable-decision-and-gateway-checks). [Runtime evidence](runtime-evidence.md) screenshots apply only to the version and pages they identify; no new runtime screenshot was added for API key management. These tests do not establish public-network behavior, shared project budgets or three-platform runtime acceptance.
