# Local gRPC Client/Server Architecture

> Status: Accepted for the TUI/GUI iteration
>
> Scope: local `agentd`, terminal client, and GPUI client
>
> Deferred: mobile clients, public endpoints, relay, OAuth, and remote control

## Decision

Lyonbrown moves application-runtime ownership into one local `agentd`. The TUI and GUI are clients of the same versioned gRPC contract and do not construct independent Kernels in the target architecture.

```text
agent TUI ──┐
            ├── gRPC / HTTP/2 / loopback ── agentd ── Kernel
agent GUI ──┘                                  ├── models
                                               ├── tools and MCP
                                               ├── workspace index
                                               └── session storage
```

The first implementation binds only to loopback. It is not a remote-control endpoint. An embedded runtime remains available during migration and for tests, but it uses the same application commands and snapshots.

## Protocol shape

Protocol Buffers are the source of truth. Unary RPCs carry commands and queries; one server-streaming RPC carries coalescible full runtime snapshots.

| Shape | Operations |
|---|---|
| Unary command | start, steer, queue, cancel, approval response |
| Unary query | handshake, health, latest snapshot |
| Server stream | latest full runtime snapshots with revisions |

Every command contains a `command_id` and `client_instance_id`. Every projected update contains a monotonically increasing `revision`. A reconnecting client first obtains a snapshot and then subscribes after its last applied revision. The v1 stream is not an event log: a slow client may receive a newer full snapshot directly, so revisions may jump. Clients deduplicate by revision and apply only newer snapshots; they do not replay or infer coalesced intermediate state.

Every command also carries the `expected_server_instance_id` obtained from the handshake. The daemon checks it atomically before reserving an idempotency key or executing the command. If an old daemon executes a command but its response is lost, that command cannot be replayed into a restarted daemon. An event-envelope revision must also equal its nested snapshot revision; a mismatch is a protocol failure.

The daemon keeps a bounded in-memory replay cache, and transport retries for one logical call reuse the same `command_id`. This guarantee only covers the same daemon instance while the entry remains cached; daemon restarts and cache eviction do not provide cross-process exactly-once delivery. A client must not blindly replay an unconfirmed non-idempotent command after `server_instance_id` changes.

The wire contract is separate from Rust domain types. `Instant`, internal errors, and implementation-specific enums must be mapped to stable protobuf values. New fields use new protobuf tags; tags are never reused.

Version one uses an explicit 16 MiB gRPC encode/decode budget. Oversized snapshots fail explicitly. If real sessions approach this budget, pagination or bounded history truncation should be introduced instead of increasing the limit without bound.

## Ownership

- `agent-protocol` owns protobuf definitions, generated messages, and protocol versioning.
- `agent-client` owns connection, command correlation, snapshot acquisition, stream subscription, and reconnect policy shared by TUI and GUI.
- `agentd` owns composition, Kernel/runtime lifetime, model loading and download jobs, workspace indexes, MCP connections, approvals, and session storage.
- TUI and GUI own only presentation state, input mapping, and client lifecycle.

Closing a client does not cancel a turn or model download. Cancellation is always an explicit RPC. The daemon arbitrates concurrent clients; the first valid response resolves an approval, and later responses receive a typed conflict.

Ordinary TUI commands run in cancellable background tasks rather than blocking the keyboard event loop. Cancel and force-stop use independent concurrent RPCs with short deadlines. On daemon shutdown, snapshot streams are cancelled before gRPC drains, so connected clients cannot keep the server alive indefinitely.

## Local endpoint and lifecycle

Version one uses gRPC over HTTP/2 on `127.0.0.1`. The daemon must never default to `0.0.0.0`. Endpoint discovery and per-launch authentication belong to a later hardening step before automatic daemon startup is enabled. Until then, the address is explicit and development-only.

Loopback is not caller authentication. The endpoint must not become the default product path or execute privileged tools until it has a random per-launch token, a discovery file readable only by the current user, and server-side token validation.

The intended startup sequence is:

1. client discovers or starts `agentd`;
2. client performs the version/capability handshake;
3. client requests the current snapshot;
4. client subscribes from that snapshot revision;
5. client sends idempotent commands through unary RPCs.

## Migration

1. Establish protobuf, reusable Rust client, and an injectable daemon service with offline tests.
2. Add a TUI transport adapter while retaining embedded mode.
3. Connect the GPUI state model through the same client.
4. Move CLI composition and model setup ownership into `agentd`.
5. Add daemon discovery, authentication, process supervision, reconnect, and revision retention.

Mobile and remote transports will extend this contract later; they do not broaden the security boundary of the local iteration.

## Current implementation status

- Complete: the protobuf/tonic contract, lossless snapshot mapping, daemon-epoch fencing, a cancellation-safe bounded replay cache, a 16 MiB message budget, a loopback-only injectable service, graceful shutdown, and revision validation.
- Complete: the shared TUI client port, embedded compatibility entry point, non-blocking command submission, and a remote adapter for handshake, initial snapshot, full-snapshot streaming, bounded retries, and reconnects.
- Complete: the GPUI native bridge, daemon-epoch/connection-state model, and live state subscription after explicit client injection; the default GUI still does not guess an endpoint.
- Not enabled yet: an `agentd` executable, default daemon startup, endpoint discovery, local authentication, or migration of model/composition ownership.
