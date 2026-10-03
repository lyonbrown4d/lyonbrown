# @lyonbrown/kernel-pi

`@lyonbrown/kernel-pi` adapts Pi Agent Core to Lyonbrown's daemon-owned runtime contract. Pi supplies the
agent loop, streaming, steering, follow-up queues, cancellation, and tool scheduling. Lyonbrown retains the
security policy, durable event boundary, resource budgets, and public API.

## Boundary

- The public exports contain only Lyonbrown types. Pi model, event, message, tool, and schema types remain in
  the private adapter modules.
- A `KernelModelAdapter` receives normalized messages and returns normalized stream events. The Pi-specific
  stream and model objects are constructed internally.
- `KernelSession` does not own the EventStore, gRPC transport, model service, or process lifecycle. The daemon
  injects those dependencies and remains the composition root.
- The injected `KernelEventSink` is the persistence boundary. Events are appended before subscribers observe
  them. In particular, `tool_started` must be durably acknowledged before a tool implementation is invoked.
- Pi is explicitly configured with `toolExecution: "sequential"`. A tool is also marked sequential and never
  replayable, preserving Lyonbrown's current ordering and side-effect policy.

## Security invariants

Model messages, tool calls, and tool results are untrusted. The adapter enforces UTF-8 byte budgets, bounded
JSON depth, finite JSON values, plain-object/prototype-pollution checks, well-formed Unicode, and removal of
ANSI, control, and bidirectional-control characters.

Tool access is default-deny:

1. The tool must exist in the injected registry.
2. Its JSON Schema, explicit risk classification, and every required permission must pass.
3. Medium- and high-risk tools require approval. A denial, timeout, disconnect, malformed response, or gate
   error is a denial.
4. Only after approval does the adapter append and await `tool_started`.
5. `SecretRef` values are resolved after durable intent, immediately before `execute`. Resolved values are
   registered with the redactor and never enter events or model-visible tool results.
6. Tool execution has one bounded timeout and no automatic retry. A missing durable result therefore remains
   an outcome-unknown operation for the outer runtime to reconcile.

Prompt text never grants permissions. Only the injected registry, policy metadata, and approval gate can
authorize a tool.

## Usage

```ts
import { createPiKernelSession } from "@lyonbrown/kernel-pi";

const session = createPiKernelSession({
  sessionId: "session-123",
  systemPrompt: "You are Lyonbrown.",
  model: localModelAdapter,
  toolRegistry,
  approvalGate,
  eventSink,
  secretResolver,
  budget: {
    maxInputBytes: 1_048_576,
    maxTurns: 32,
    maxSteps: 64,
    maxOutputBytes: 1_048_576,
    maxToolResultBytes: 262_144,
    maxValueDepth: 32,
    toolTimeoutMs: 30_000,
  },
});

session.subscribe((event) => projectForClients(event));
await session.startTurn("Inspect the workspace");
```

`steer()` queues user input for the next Pi steering point, `followUp()` queues work after the current run would
otherwise stop, and `cancel()` aborts the active Pi run. Each `startTurn()` also accepts an external
`AbortSignal`.
