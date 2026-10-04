# Local gRPC Client/Server Architecture

Lyonbrown has one product topology:

```text
agent TUI ── authenticated local gRPC ── agentd
                                         ├── durable Runtime and Pi kernel
                                         ├── EventStore, tools, MCP, secrets
                                         └── node-llama-cpp and other models
```

The daemon is the sole authority for sessions and execution. The TUI owns input,
rendering, locale, theme, and short-lived presentation preferences only. Closing
the TUI does not stop a turn or a model download already accepted by the daemon.

## Transport and discovery

The transport is local gRPC using generated protobuf bindings. The daemon
publishes a protected, scoped discovery descriptor only after its server is
ready. The client verifies descriptor ownership and scope, reads credentials
through the native capability boundary, performs a versioned handshake, and
subscribes to an authoritative snapshot stream.

Endpoint strings and bearer tokens never appear in command-line arguments,
logs, events, crash reports, or UI text. Authentication failure, identity
mismatch, malformed discovery data, and unsupported protocol versions fail
closed.

## Connection lifecycle

The TUI renders immediately in `connecting` state. Discovery, optional daemon
launch, authentication, handshake, and subscription run in the background.
Reconnect uses bounded backoff and accepts state only for the current daemon
epoch and monotonically newer revision. A stale snapshot may remain visible for
diagnosis, but mutating daemon actions stay disabled until fresh state arrives.

## Command model

Commands carry command identity, session identity, expected daemon/runtime/model
epochs where applicable, and bounded payloads. The daemon validates them before
durable append. Duplicate delivery is idempotent; stale epochs, invalid state,
oversized messages, and unauthorized actions are rejected without partial state.

The TUI supports start turn, cancel, steering, queued follow-up, approval, model
download/cancel/retry, and session/model selection only through this contract.
There is no direct Runtime call or embedded fallback.

## Model setup

Daemon startup and TUI startup never force a model download. Cache inspection is
background work. A missing model produces explicit setup state and disables only
model-dependent submission. The user may start a smaller default model download,
observe progress, cancel while allowed, retry a failure, or exit.

The daemon owns download locks, byte limits, checksum verification, secure staging,
atomic activation, model epochs, and llama.cpp loading. The TUI renders the
versioned state and sends explicit commands; it never opens GGUF files.

## Failure and shutdown

Transport failures are sanitized and bounded. Stream cancellation and server
shutdown are deadline-bound so a dead client cannot retain daemon resources.
The client may restart independently and recover from the latest durable snapshot.
