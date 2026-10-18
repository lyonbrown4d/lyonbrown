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

CLI and Desktop forward their default-disabled `native-computer` feature to `agent-computer-runtime/native`. Native SDKs require the explicit `native` feature. `agent-computer-runtime/native` enables `agent-computer/native` and the `attach_native` factory. Default builds retain the protocol, engine and injectable `attach_host` without capture or input SDKs. The daemon uses this default dependency path.

Native target discovery reads metadata without capturing pixels or requesting OS permission. It checks the interactive session and existing screen permission before and after enumeration. Minimized and zero-sized targets are omitted; catalogs exceeding 512 entries are rejected. Labels have control characters removed and are limited to 512 characters. `DesktopTarget` Debug output omits labels that may contain private content. Window descriptors retain the process ID; attachment checks window and process IDs to reject replacement windows. Titles and geometry are checked against fresh observations, rather than treating the old catalog as the current screen.

`agent-computer` encapsulates screen observation, input and desktop leases. `agent-computer-runtime` retains prepared actions in process and checks their daemon Session, Run and `owner_epoch` binding. Execution messages refer to the original preparation, action identity and content digest; they cannot replace the input. After approval, the runtime observes again using the original lease and limits. Changed content, an invalid lease or cancellation prevents input. The preparation is consumed before refresh or input, so a failure cannot replay that action.

The client-side library can embed a native host through `attach_native`. Before attachment it checks the authenticated daemon node and selected session ownership. The handshake fixes the desktop scope; the daemon verifies identity again and records the session's `owner_epoch`. A session permits one host attachment. Model tools pin the attachment generation. Every operation checks Run state and session ownership before dispatch and after response; a reconnect cannot replace the host already selected by a run. Attaching grants no capture or input permission.

An attachment permits one pending command. Requests for the same attachment generation queue serially, so renewal cannot preempt a dispatched command. The wait supports cancellation and a 35-second deadline; dispatch checks the attachment generation again. The daemon checks replies against the request, host, attachment generation, Session, `owner_epoch`, private protocol version and expected reply type, then validates lease, scope, image, window and AX node bounds. Invalid replies or disconnects cancel the attachment without replaying input. Native calls and message codecs run on a blocking worker. Calls already inside the OS still need to drain; cancellation does not terminate their threads. A native host holds a file lock in the user data directory keyed by the OS graphical session. Changing daemon node identity or a configurable client data path cannot split this lock. The executing worker retains it until the call drains.

Native runtimes in the same process share a desktop lease registry. Token-specific revocation validates and cancels the exact live token under one lock; an expired controller cannot revoke its replacement's lease. Private postcard protocol version 2 includes exact-token `RenewLease`. Renewal extends only the current unrevoked, unexpired token; it cannot acquire another token or restore expired control. The packet limit is 68 MiB, accommodating up to 64 MiB of RGBA pixels plus metadata. Oversized frame prefixes are rejected before payload allocation. The daemon limits capability replies to 4 KiB, observation replies to 68 MiB, and other replies and requests to 16 KiB. The daemon still owns durable approvals, execution intents and replay protection across process restarts; in-process records cannot replace persistent records.

The daemon provides Harness `computer_observe` and `computer_act` through this authenticated connection. See [computer tools](tools.md#model-computer-tools) for availability checks, exact approval and durable receipts. CLI/TUI and Desktop expose explicit target selection and session attachment; see [native computer attachment](clients.md#native-computer-attachment). The configuration switch alone does not create a native host. These components do not establish client-to-Harness native-control acceptance. The macOS native factory returns `SessionStateUnknown` when its public session probe cannot establish unlocked state, without starting capture or input. Windows and Linux adapters also require their platform permissions and actual runtime acceptance.
