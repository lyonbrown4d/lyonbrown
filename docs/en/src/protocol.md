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

## Typed execution events and input during a Run

`agent_core::events::AgentEvent` is the application-owned execution contract. `PhaseChanged` carries an `ExecutionPhase` and optional `ModelIdentity`; tool start/result, usage, queue state and Run state have explicit fields. Arbitrary tool arguments remain JSON values. No Harness, Provider SDK or UI types cross the boundary. TUI and Desktop share enum decoding rather than infer phases from audit JSON.

The existing `Event` Run ID, cursor, timestamp and `kind/payload_json` envelope remain compatible. New facts use `kind=agent_event` and an internally tagged enum payload. Legacy runtime and Run-state events are adapted centrally; unknown kinds retain only their name, while malformed known payloads return a decoding error. `record_appended` and `entry_appended` remain audit data, not a client phase protocol. The provider request hook emits `ConnectingModel`, meaning a request is being initiated, not that a TCP connection has succeeded.

`SubmitRunInput` requires a Run ID, owner epoch, UUID request ID, explicit `STEER/FOLLOW_UP` kind and a nonempty prompt up to 1 MiB. Queue acknowledgement commits atomically with the Harness snapshot, events and Outbox. The same request identity and content return the original receipt; changed content or target Run is rejected. A Session cannot start another concurrent Run. `ListRunInputs` returns input records for the specified Run's Session, including retained inputs across Runs, while preserving each receipt's original Run ownership. Snapshots include all pending entries and at most 100 terminal records, with `history_truncated` marking omitted history. Prompt previews are bounded to 4096 bytes and flagged by `prompt_truncated`; previews must never be resubmitted as complete requests. Pending input is limited to 100 entries and 1 MiB total. `CancelRunInput` checks the entry and current Session epoch, then uses its currently attached Runtime. Consumed input is never reported as withdrawn. Pending input left across a boundary never automatically starts a new Run; disconnects never replay submission. A queue without an attached Runtime after restart supports inspection, not a claim of restored immediate withdrawal.

The runtime adapter converts SDK records into typed tool, usage and queue facts before committing them with the snapshot and Outbox. The daemon supplies an explicit Run ID instead of guessing the latest Run. Cancellation first commits a `CancelRequested` intent before calling the Harness; only durable cancellation or a committed consumption fact resolves it. Failure closes input, stops execution and fences the Run as `uncertain`, retaining an earlier completed result for inspection. Unresolved intents reject new StartRun requests and rebuild that fence on startup without executing the queue. Capabilities are advertised as `events.semantic` and `runs.input`; unsupported older daemons never fall back to ordinary StartRun.

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

The daemon provides Harness `computer_observe` and `computer_act` through this authenticated connection. See [computer tools](tools.md#model-computer-tools) for availability checks, exact approval and durable receipts. CLI/TUI and Desktop expose explicit target selection and session attachment; see [native computer attachment](clients.md#native-computer-attachment). The configuration switch alone does not create a native host. These components do not establish client-to-Harness native-control acceptance. See the macOS session checks and limits below. Windows and Linux adapters also require their platform permissions; actual runtime acceptance on those platforms remains incomplete.

macOS reads the calling process's CoreGraphics session and requires it to be on the console with login complete and a `CGSSessionUniqueSessionUUID` that parses as a non-nil UUID. Using maintained `objc2` bindings, it also reads the IORegistry root and requires an explicit `IOConsoleLocked = false`. Either `IOConsoleLocked` or `CGSSessionScreenIsLocked` being `true` returns `Locked`. A missing or malformed `IOConsoleLocked`, or a missing or invalid session UUID, returns `SessionStateUnknown`. Each session check reads and validates snapshots before and after the lock probe; a changed snapshot is rejected. The lock fields are macOS implementation fields. This is the project's chosen compatibility policy, not an Apple-documented lock-state guarantee.

Once the native backend observes a lock, unreadable session state or changed session identity, it stays revoked. Unlocking cannot restore control through that backend; a fresh explicit attachment is required. Session checks, screen capture and input remain separate OS calls; before/after validation does not guarantee atomic capture and input. See `crates/agent-computer/src/native/macos.rs`, `session.rs` and `mod.rs`.
