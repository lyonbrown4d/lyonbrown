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

| Shape         | Operations                                     |
| ------------- | ---------------------------------------------- |
| Unary command | start, steer, queue, cancel, approval response |
| Unary query   | handshake, health, latest snapshot             |
| Server stream | latest full runtime snapshots with revisions   |

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

The wire contract stays separate from TypeScript domain types. New protobuf fields
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

These filesystem semantics are release gates for the TypeScript production
entry point. The POSIX provider must supply dirfd/openat, `O_NOFOLLOW`,
owner-only permissions, atomic publication, and kernel locks. The Windows
provider must use protected current-user-only DACLs, reject reparse points, and
validate descriptors, locks, and file identity through handles. A missing or
failed provider must fail closed on every platform; there is no path-based or
permissive fallback.

## Ownership and Node/TypeScript ecosystem choices

- `protocol`: protobuf definitions, generated types, and versioning;
- `agent-client`: authenticated connections, command correlation, Snapshots,
  streams, retries, and reconnects;
- `local-control-plane`: tokens, discovery, secure directories, and both lock contracts;
- `agent-launch`: discover/reuse/spawn/readiness coordination;
- `agent-runtime`: configuration, composition, models, and runtime services;
- `agentd`: the only composition root and gRPC server;
- TUI/GUI: interaction and presentation only.

The implementation prefers established Node packages: `@grpc/grpc-js` for gRPC,
`ts-proto` for generated protocol types, Pi Agent Core for the agent loop, and
`better-sqlite3` for the local event store. Node 24 does not expose the complete
secure filesystem API, so the native provider uses an exactly pinned Koffi
binding for a narrow OS surface while retaining `node:fs` for safe fd operations.
Project code is reserved for Lyonbrown-specific scope, epoch, lifecycle,
persistence ordering, and UI semantics rather than reimplementing gRPC,
databases, or the agent loop.

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

## TypeScript migration status

On the current migration branch, the protocol, domain model, UI projection,
SQLite EventStore, Pi Kernel adapter, durable `agent-runtime`, authenticated
`agent-client`, and Node `agentd` gRPC adapter are covered by TypeScript tests.
The Windows native provider now supplies secure run directories, locks, atomic
publication, scoped paths, and lifetime anchoring for the database and WAL/SHM
artifacts. `agent-launch` covers discover/reuse/spawn/readiness and epoch
fencing. `local-control-plane` remains fail closed when its native capability is
missing.

`packages/local-model` now provides required `node-llama-cpp`, the pinned
default preset, explicit background download, validation, and load lifecycle.
A real Windows `NativeModelStorageCapability`, the releasable daemon composition
root, system process/discovery adapters, and TUI/GUI product integration remain
explicit release blockers.

> The remainder of this section is a product acceptance baseline and a behavior
> oracle from the previous implementation. It does not mean those capabilities
> are already exposed by the new TypeScript product entry point. Migration does
> not add an embedded mode, Rust bridge, or dual-write compatibility path.

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

The shared Snapshot contract now also has an optional, UI-neutral configured
model catalog and an active/pending session policy. The catalog is projected
from the providers registered by the daemon composition root, accepts at most
128 unique exact routes, and carries tier, locality, context/output limits, capability
flags, and daemon-computed Worker selectability. Provider probes run
concurrently with a per-provider deadline. A timeout or operational provider
failure publishes no catalog instead of blocking startup. Identity mismatches,
duplicate/invalid routes, and catalog overflow abort activation as invariant
violations rather than being deduplicated or truncated. A successful probe
seals the registry, so the scheduler and published catalog use the same immutable
capability snapshot for that daemon epoch. `None` means no catalog was published, while an
explicit empty catalog remains distinguishable. Catalog entries mean
configured/selectable, not online or ready, and never contain endpoints,
secret references, GGUF paths, or raw provider failures.

For this contract revision, a route is selectable as the session Worker only
when it declares the Worker tier plus tool and streaming support. Other routes
remain visible with a stable disabled reason.

`Auto` means there is no session override and the daemon continues to apply
its base provider/model configuration and routing policy; it does not mean that every catalog
entry is eligible. Until the selection command exists, active is always `Auto`
and pending is always absent. The contract and projection are implemented, but the model
selection command is still unsupported and neither TUI nor GUI has the real
picker yet.

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
