# `agentd` Composition Root

`apps/agentd` is the only production composition root. Dependency injection is
kept at the process boundary; domain objects do not resolve services from a
container.

## Ownership

The daemon constructs and owns:

- scoped run and data directories plus native security capabilities;
- lifecycle lock, discovery descriptor, endpoint, and authentication token;
- EventStore, durable Runtime, Pi kernel session, tools, MCP, and secrets;
- managed `node-llama-cpp` model storage and lifecycle;
- authenticated gRPC services and snapshot streams.

The TUI receives only narrow `agent-launch`, `agent-client`, and `ui-model`
interfaces. It never receives a database handle, model path, raw bearer token,
provider secret, or Pi object.

## Startup order

1. Parse and validate typed process configuration.
2. Derive the workspace scope from stable filesystem identity and effective
   configuration fingerprint.
3. Open protected run/data directories and acquire the daemon lifecycle lease.
4. Open and recover EventStore, then construct Runtime and the Pi adapter.
5. Start the authenticated gRPC server on a private endpoint.
6. Publish the discovery descriptor last, only after the service is ready.

Any failure before publication remains undiscoverable and is cleaned up in
reverse ownership order. The process never falls back to an embedded Runtime.

## Shutdown order

The daemon stops accepting RPCs and streams, performs bounded graceful gRPC
shutdown, forces closure after the deadline, disposes Runtime and model
resources, closes EventStore, releases leases and directory anchors, then
removes discovery state. Every cleanup step is attempted; diagnostics expose a
sanitized first failure without leaking credentials or local secret paths.

## Test seam

Production composition injects native capabilities explicitly and fails closed
when a platform implementation is unavailable. Tests use ports and fakes at
the same boundary; test helpers are never selected by production defaults.
