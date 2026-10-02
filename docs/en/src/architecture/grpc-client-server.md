# Local gRPC Client/Server Architecture

> Status: accepted and under implementation verification
>
> Scope: local `agentd`, terminal client, and GPUI client
>
> Deferred: mobile clients, public endpoints, relay, OAuth, and remote control

## Decision

Lyonbrown uses one local Client/Server architecture. The TUI and GUI are clients
of the same versioned gRPC contract. Clients no longer construct a Kernel, and
there is no embedded product mode or unauthenticated transport compatibility
entry point.

```text
agent TUI ──┐
            ├── gRPC / HTTP/2 / loopback ── agentd ── Kernel
agent GUI ──┘                                  ├── llama-cpp and other models
                                               ├── tools and MCP
                                               ├── workspace index
                                               └── session storage
```

`agentd` is the sole owner of the runtime, models, tools, MCP connections,
indexes, approvals, and sessions. TUI and GUI own only presentation state,
input mapping, and client lifetime. Closing one client therefore does not
implicitly cancel a running turn or model job; cancellation is an explicit RPC.

This phase permits loopback communication only. The local endpoint is not a
remote-control endpoint.

## Protocol shape

Protocol Buffers are the source of truth. Unary RPCs carry commands and queries;
one server-streaming RPC carries coalescible full runtime snapshots.

| Shape | Operations |
|---|---|
| Unary command | start, steer, queue, cancel, approval response |
| Unary query | handshake, health, latest snapshot |
| Server stream | latest full runtime snapshots with revisions |

Every command contains a `command_id`, `client_instance_id`, and the
`expected_server_instance_id` returned by the handshake. Before a command enters
the native client queue, the caller binds it to the currently accepted
`server_instance_id`; the wire request preserves that value as the expected
epoch instead of consulting a later connection state. The daemon checks the
server instance atomically before reserving an idempotency key or executing the
command. It returns a typed `EpochMismatch` detail on disagreement, which the
client maps to `StaleEpoch`. An old-epoch command therefore fails closed and
cannot cross into a replacement Runtime merely because its response was lost.

A successful command receipt is accepted only when `accepted` is true and both
its `protocol_version` and `command_id` match the request. A rejected or
mismatched receipt is a protocol error, never evidence that the command was
accepted.

Every projected update contains a monotonically increasing `revision`. A client
obtains a Snapshot before subscribing after its last applied revision. The v1
stream is not an event log: a slow client may receive a newer full snapshot
directly, so revisions may jump. Clients apply only newer revisions and do not
infer coalesced intermediate state. An event revision that differs from its
nested Snapshot revision is a protocol error.

The daemon keeps a bounded in-memory replay cache. Transport retries for one
logical call reuse the same `command_id`. This guarantee covers only the same
daemon while its entry remains cached; it is not cross-process exactly-once.
Clients must not blindly replay an unconfirmed non-idempotent command after the
`server_instance_id` changes.

The wire contract stays separate from Rust domain types. New protobuf fields
use new tags, and published tags are never reused. Version one has a 16 MiB gRPC
encode/decode budget. Oversized Snapshots fail explicitly; pagination or bounded
history truncation should be added before raising that budget without a bound.

## Local authentication and scope

`agentd` listens only on a randomly assigned `127.0.0.1` port. Loopback limits
network reachability but does not authenticate the caller, so every product RPC
requires a bearer token generated for each daemon launch. Public
unauthenticated client and server APIs have been removed. Requests with a
missing or incorrect token are rejected before entering the application service.

The discovery descriptor contains only the required connection material:

- the loopback address, PID, and `server_instance_id`;
- the per-launch bearer token;
- a `scope_fingerprint` derived from the workspace and runtime configuration;
- the schema version and transport type.

The client and daemon independently compute the runtime configuration
fingerprint. The workspace filesystem identity and configuration fingerprint
derive the scope, so equivalent path spelling does not fork a second daemon.
The daemon validates the configuration fingerprint at startup and
returns the scope fingerprint in its handshake. A client reuses the instance
only when discovery, handshake, and expected scope all agree, preventing a
connection to a daemon for another workspace or configuration.

## Runtime epoch transitions

`agentd` uses an in-process epoch supervisor to replace a Runtime. After model
installation, the supervisor builds the replacement while the old control
plane remains available. On success it starts the new loopback endpoint,
atomically publishes the new discovery descriptor, and only then gracefully
stops the old endpoint and Runtime. Explicit graceful shutdown is followed by
abort cleanup so exceptional tasks cannot leak across epochs.

Each transition rotates the endpoint address, bearer token, and
`server_instance_id`. The workspace scope, daemon PID, and daemon-lifetime
`agentd.lock` stay unchanged. Clients rerun discovery and the authenticated
handshake, then resynchronize from a Snapshot; they do not blindly replay an
unconfirmed old-epoch command into the replacement.

If building the Runtime or starting its endpoint fails, the supervisor keeps
the old control plane online, publishes a recoverable failure, and permits an
explicit retry. The old endpoint may exit only after the new descriptor has
been published successfully. This is the rollback and fail-closed boundary for
the local control plane.

## Discovery, reuse, and startup

Every client entry point goes through the launch coordinator:

1. Read and strictly validate the owner-only discovery descriptor.
2. Use its token for a deadline-bound gRPC handshake.
3. Reuse the daemon when instance ID, scope, and configuration match.
4. When no instance is reusable, compete for the startup lock; the winner
   spawns `agentd`.
5. Wait for discovery publication and an authenticated handshake. Other clients
   then reuse the same instance.
6. Obtain the current Snapshot, start the subscription, and then send commands.

The native client driver performs this sequence in the background. TUI and GUI
render the `Connecting` state immediately instead of blocking their interface.
After a daemon restart, reconnect re-runs discovery so a new endpoint, bearer
token, and server epoch replace the old command and event channels together.

Startup coordination and daemon ownership use separate locks:

- `agentd.start.lock` elects one client to start the daemon and is released
  after readiness;
- `agentd.lock` is held for the daemon lifetime and ensures one owner per scope.

Separating the locks prevents a client waiting for readiness from blocking the
daemon's lifetime lock. Locks and discovery live in the same secure run
directory. The descriptor is published atomically, and the token is not passed
through command-line arguments or environment variables.

Unix enforces owner-only runtime directories and files, atomic discovery
publication, and process locks. Windows enforces the equivalent boundary with
protected current-user-only DACLs, rejects reparse points, and validates every
descriptor and lock handle before use. Neither platform falls back to
permissive access or an unauthenticated daemon.

## Ownership and Rust ecosystem choices

- `agent-protocol`: protobuf definitions, generated types, and versioning;
- `agent-client`: authenticated connections, command correlation, Snapshots,
  streams, retries, and reconnects;
- `agent-local`: tokens, discovery, secure directories, and both locks;
- `agent-launch`: discover/reuse/spawn/readiness coordination;
- `agent-runtime`: configuration, composition, models, and runtime services;
- `agentd`: the only composition root and gRPC server;
- TUI/GUI: interaction and presentation only.

The implementation prefers established Rust crates: `tonic`/`prost` for gRPC
and protobuf, `tokio` for async runtime, processes, and signals, `serde` for the
bounded schema, `getrandom`, `secrecy`, and `subtle` for token generation,
secret wrapping, and constant-time comparison, `file-id` for cross-platform
workspace identity, and `rustix` for the Unix file security boundary. Project
code is reserved for Lyonbrown-specific scope,
lifecycle, and UI semantics.

## Model and download boundary

`llama-cpp` is an `agentd` default capability and default feature, not a TUI or
GUI responsibility. A missing model never triggers a startup download. The
daemon exposes an actionable state while the client remains usable for
configuration or an explicit background download. The default managed preset
is `qwen3-1.7b-q4_k_m`. A missing explicitly configured `model_path` is not
eligible for a managed download; the user must repair the configuration or
choose another model.

The download control plane uses unary `StartModelDownload` and
`CancelModelDownload` calls. Progress and results remain on the single
versioned `RuntimeSnapshot` stream instead of creating a second model event
stream. Managed-model state carries its source, phase, allowed actions, byte
progress, failure, and `operation_id`. Phases cover lock waiting, checking,
downloading, verification, installation, activation, ready, cancelled, and
failed. The recovery action after failure is Retry, never resume.

The job belongs to the daemon, so closing a TUI or GUI does not cancel it.
Cancel must carry the current `operation_id`; a stale operation cannot cancel a
newer job. Cancellation becomes TooLate after the job enters `Installing`,
because atomic installation cannot be interrupted. The daemon catalog owns the
model ID, URL, digest, and destination; clients cannot submit arbitrary URLs or
paths.

## Current implementation boundary

The protocol, authentication, scope handshake, secure discovery, two-lock
design, launch coordinator, `agentd` composition, reconnecting shared client,
and unified C/S entry points for TUI and GUI are implemented and covered by the
workspace test suite. Explicit daemon-side background downloads, progress on
the single Snapshot stream, in-process epoch replacement, and fail-closed
command epoch fencing are also implemented.

Both clients now expose the same model-setup loop over those Snapshot/RPCs:
explicit start or retry, progress, cancellation where allowed, persistent
failure details, and English/Simplified Chinese copy. The GUI places its setup
card between the conversation and Composer, projects the actual phase and
progress in the Runtime rail, and provides a top-bar English/Chinese switch.
The TUI selects its startup locale from the system, offers a global `F2` switch
across setup, approval, and conversation, and handles `/language` locally
without sending a daemon command or mutating transcript/Session state.
Stale state remains visible for diagnosis in both clients, but setup actions
are disabled until a fresh Snapshot for the accepted epoch arrives. Neither
client implements a downloader. The TUI now has one bilingual, ten-command
catalog shared by slash parsing, the `Ctrl-P` palette, and help. Unimplemented
commands are disabled rather than simulated, unknown slash input is rejected
locally, and Model Setup/Approval input takes priority over overlays. Accepted
snapshots now also feed a read-only TUI Activity Inspector. `Ctrl-O`
opens details keyed by stable turn/step IDs; accepted revisions update its
projection and cache, while repeated revisions and epoch-mismatched snapshots
cannot mutate it. A disconnect marks the last accepted evidence as stale cached
instead of erasing it, and a Session or daemon-epoch change closes and clears
the old selection. Detail generation is cached and bounded by field, collection,
JSON, line, and total-character budgets.

This MVP exposes route and fallback reasons, context and token usage, tool
arguments/results/duration, outcomes, and relevant IDs in English and Chinese.
It is not a complete event trace: timestamps, strict event history, historical
`ToolPreview`/capabilities, and complete diagnostics are not present. Clipboard
copy is also not implemented. Remaining frontend work includes the M12 TUI
session/model selectors, Theme overlay and preference persistence, plus the M13
complete GUI transcript, streaming, approval, cancellation, and resume loop.
M12 is not complete.

Mobile and remote control remain deferred. A future transport and authentication
system must not reuse the local discovery token or expose this loopback service
before remote identity, authorization, and encryption boundaries exist.
