# Protocol and platform capabilities

The public contract is `proto/agent/v1/agent.proto`, using package `agent.v1`. Prost and Tonic generate Rust types, which `agent-protocol` exposes to servers and clients. Versioned messages contain no concrete model-library, Harness or UI types.

## Service boundaries

| Service | Message scope |
| --- | --- |
| `ClientService` | Node information, sessions, runs, events, cancellation and approvals |
| `WorkspaceService` | File reads, revision-checked writes, whole-workspace diffs and watching |
| `OrchestrationService` | Local Master dispatch, paged queries, reconciliation, cancellation and result acceptance |
| `TaskService` | Worker submission, queries, events, cancellation and result acceptance |
| `PeerService` | Node information, pairing and trust revocation |
| `BrowserService` | Browser spaces, tabs, observations, actions and human takeover |
| `BrowserRuntimeService` | Authenticated bidirectional attachment for the embedded client runtime |
| `ComputerRuntimeService` | Authenticated native execution attachment bound to a daemon session |
| `ControlService` | Registration, resolution, pairing and bidirectional relay messages |

This table lists protocol contracts, not a feature-availability checklist. Unwired services and platform adapters must report unavailability explicitly rather than return successful placeholders.

`agent-client` adds `authorization: Bearer …` metadata. Clients provide the endpoint and token. The default endpoint is local HTTP. Public deployment additionally requires transport protection, node identity, authorization and deployment configuration; local token validation does not establish public-deployment acceptance.

See [Connect peers and Control Relay](network.md) for identity verification, pairing scopes, direct and outbound Relay configuration, and failure recovery.

## Important fields

| Field | Meaning |
| --- | --- |
| `session_id` | Durable session identity |
| `owner_node_id` / `owner_epoch` | Session or task writer and ownership generation |
| `idempotency_key` | Deduplication identity for the same logical submission |
| `attempt_id` | One worker execution attempt, distinct from its task |
| `cursor` / `after_cursor` | Event position and incremental-read starting point |
| `deadline_ms` / `expires_at_ms` | Task deadline and approval expiry in milliseconds |

## Platform adapters

The workspace separates browser, computer operation, distributed connection and local inference crates. Adapters must report native dependencies and available capabilities on Windows, macOS and Linux. A common protocol does not remove differences in SDKs, display servers, permissions or physical devices.

The local-inference boundary is inference adapters for `mistral.rs` and `llama-cpp-2`; inference libraries do not own sessions, tools or the agent loop. See [Models and execution decisions](models.md) for configuration and authority, and [Development and verification](development.md) for separate real-model checks.

BrowserRuntime is a crate embedded in the client; CEF helpers use the client’s private runtime mode, with browser execution isolated from GPUI UI state. Browser capability is unavailable without a matching CEF SDK and runtime resources. Persistent profile storage and restoration of live pages have different semantics; a profile directory does not establish restored page state.

## Native desktop execution boundary

Native SDKs require the explicit `native` feature. `agent-computer-runtime/native` enables `agent-computer/native` and the `attach_native` factory. Default builds retain the protocol, engine and injectable `attach_host` without capture or input SDKs. The daemon uses this default dependency path.

`agent-computer` encapsulates screen observation, input and desktop leases. `agent-computer-runtime` retains prepared actions in process and checks their daemon Session, Run and `owner_epoch` binding. Execution messages refer to the original preparation, action identity and content digest; they cannot replace the input. After approval, the runtime observes again using the original lease and limits. Changed content, an invalid lease or cancellation prevents input. The preparation is consumed before refresh or input, so a failure cannot replay that action.

The client embeds a native host and connects through `attach_native`. Before attachment it checks the authenticated daemon node and selected session ownership. The handshake fixes the desktop scope; the daemon verifies identity again and records the session's `owner_epoch`. A session permits one host attachment. Every capability inspection checks Run state and session ownership before dispatch and after response. Attaching grants no capture or input permission.

An attachment permits one command in flight, with native calls and message codecs running on a blocking worker. Duplicate requests, overlapping commands, malformed messages or disconnects cancel the attachment without replaying input. Calls already inside the OS still need to drain; cancellation does not terminate their threads. A native host holds a file lock in the user data directory keyed by the OS graphical session. Changing daemon node identity or a configurable client data path cannot split this lock. The executing worker retains it until the call drains.

Native runtimes in the same process share a desktop lease registry. Token-specific revocation validates and cancels the exact live token under one lock; an expired controller cannot revoke its replacement's lease. Private postcard messages are limited to 68 MiB, accommodating up to 64 MiB of RGBA pixels plus metadata. Oversized frame prefixes are rejected before payload allocation. The daemon still owns durable approvals, execution intents and replay protection across process restarts; in-process records cannot replace persistent records.

The daemon attachment interface exposes read-only capability inspection; model tools do not execute native input through it. These components do not establish client-to-Harness native-control acceptance. The macOS native factory returns `SessionStateUnknown` when its public session probe cannot establish unlocked state, without starting capture or input. Windows and Linux adapters also require their platform permissions and actual runtime acceptance.
