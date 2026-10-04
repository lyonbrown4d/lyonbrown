# TypeScript Composition Root

Lyonbrown has one production composition root in the Node/TypeScript `agentd`.
Domain packages express dependencies through constructors, factories, and
consumer-owned narrow interfaces. They do not use a service locator or depend on
a DI container. The TUI and GUI are gRPC clients and never construct a Kernel or
Runtime.

## Composition boundary

Startup proceeds in this order:

1. Resolve and freeze runtime configuration, workspace identity, and path policy.
2. Acquire the secure run directory and daemon-lifetime lock.
3. Open the EventStore and recover the runtime projection from durable events.
4. Construct model, tool, approval, and Pi Kernel adapters.
5. Construct `agent-runtime`, then start the authenticated loopback gRPC server.
6. Atomically publish the discovery descriptor and only then become ready.

```text
configuration + EpochIdentity + secure local capabilities
                           │
                           ▼
                  agentd composition root
                           │
                           ├── EventStore
                           ├── Pi Kernel adapter
                           ├── Model / Tool / Approval ports
                           ├── AgentRuntime
                           └── authenticated gRPC server
```

Composition uses explicit factories rather than a container:

- `createAgentRuntime({ store, kernelFactory, clock, ids })`
- `createAgentGrpcServer({ host, token, serverInstanceId, scopeId })`
- `createAgentClient({ binding, deadlines })`

Concrete Pi, SQLite, and gRPC types remain inside their adapter packages. Domain
events and runtime projections do not depend on them. The production entry point
does not export a generic resolver, unauthenticated server, embedded runtime, or
test bootstrap.

## Lifecycle and failure boundaries

`agentd` exclusively owns models, tools, sessions, and durable state. A command
may publish a Snapshot or successful receipt only after persistence succeeds. If
a side effect outcome cannot be established, the session becomes terminal instead
of guessing that execution may continue.

`EpochIdentity` injects `server_instance_id`, scope, and bearer token as one
immutable launch identity. Runtime replacement constructs a new composition and
rotates the epoch rather than hot-swapping global singletons.

The secure native provider is a hard production dependency. Without it, startup
fails closed with `NATIVE_SECURITY_PROVIDER_REQUIRED`. There is no fallback to
path-based filesystem operations, `proper-lockfile`, raw endpoint/token flags, or
a Rust bridge.

## Test replacement strategy

Tests inject an in-memory EventStore, fake Kernel/model, fake clock/IDs, and a fake
control-plane provider at consumer boundaries. gRPC tests still use a real
`grpc-js` loopback transport to verify authentication, epoch fencing, replay, and
stream behavior. Fakes are not exported as product entry points.

During migration, the previous Rust code may remain on the branch as a behavior
oracle. It is not part of the final runtime and does not define a compatibility
mode. Once the equivalent TypeScript vertical slices pass their parity tests, the
old implementation is removed as a whole.
