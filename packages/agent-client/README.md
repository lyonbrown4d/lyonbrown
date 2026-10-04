# @lyonbrown/agent-client

Authenticated gRPC client boundary shared by the Lyonbrown TUI and GUI.

The public API accepts an `EpochBinding` produced by `@lyonbrown/local-control-plane`; it does not
accept raw endpoint/token pairs. Every RPC carries the binding's bearer credential. A handshake must
validate the protocol version, daemon instance, and scope before commands, snapshots, or events are
used.

```ts
import { createAgentClient } from "@lyonbrown/agent-client";

const client = createAgentClient({
  binding,
  deadlines: { unaryMs: 30_000, commandAttempts: 3, retryBackoffMs: 100 },
});

await client.handshake({ clientName: "agent", clientVersion: "0.1.0" });
await client.enqueue({ type: "start-turn", text: "Explain this repository" });
```

`enqueue` freezes the command ID and expected daemon epoch before transport. Retryable transport
failures reuse that command ID only while the binding remains in the same epoch. A replacement epoch
fails the command closed, so a non-idempotent command is never replayed into another daemon.

Snapshots and events are mapped into Lyonbrown-owned DTOs. Generated protobuf and `grpc-js` types do
not cross the package entry point. Event subscriptions are async iterables: stop iteration or abort
the supplied signal to cancel the underlying gRPC stream while preserving Node stream backpressure.
