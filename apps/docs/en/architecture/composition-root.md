# `agentd` Composition Root

`apps/agentd` is the only production composition root. Dependency injection is
kept at the process boundary; domain objects do not resolve services from a
container. The production launcher and executable entry point now use this
composition; there is no second in-process or compatibility architecture.

> Verified implementation checkpoint: 2026-10-07. The built launcher,
> independent daemon startup, and production-composition real-GGUF black boxes
> pass; final release-artifact verification remains pending.

## Ownership

The daemon constructs and owns:

- scoped run and data directories plus native security capabilities;
- lifecycle lock, discovery descriptor, endpoint, and authentication token;
- EventStore, durable Runtime, Pi kernel session, and capability policy;
- managed `node-llama-cpp` model storage and lifecycle;
- authenticated gRPC services and snapshot streams.

The TUI receives only narrow `agent-launch`, `agent-client`, and `ui-model`
interfaces. It never receives a database handle, model path, raw bearer token,
provider secret, or Pi object. Snapshot ordering uses a daemon-owned monotonic
transport revision rather than exposing EventStore revisions directly; this
also orders model-only and activation updates.

The production kernel composes workspace-confined `read`, `write`, and `edit`
tools through a native-required `@openclaw/fs-safe` root plus an explicit
approval gate. Reads are bounded to 1 MiB and writes use confined atomic
replacement. Exact one-shot/session approval decisions are durable domain
events. Its secret resolver still rejects and MCP is not connected. Tools and
approval remain daemon-owned; the TUI only projects state and submits the
user's decision.

## Workspace import boundary

Hand-written TypeScript does not use relative module specifiers. It imports
through `@lyonbrown/...` root aliases. Each workspace owns its `/internal/*`,
`/test/*`, and `/script/*` namespaces; repository gates reject access from a
different workspace. Cross-package imports must use declared public package
entries.

Vitest resolves source aliases through `vite-tsconfig-paths`. Production
TypeScript builds run `tsc-alias` after compilation to emit Node ESM paths with
`.js` extensions, and private aliases must not remain in build artifacts. The
Git-ignored protocol output remains generated and owned by Buf; it is not a
precedent for hand-written code to bypass the import boundary.

## Background work and scheduling

`agentd` does not build a business scheduler. Native timers currently serve
only RPC deadlines, bounded shutdown waits, cancellation propagation, and
finite backoff; they do not provide restart-safe job semantics. When a real
scheduled or background workload appears, selection follows its semantics:
evaluate [Croner](https://github.com/Hexagon/croner) for process-local cron,
[`p-retry`](https://github.com/sindresorhus/p-retry) for bounded async retry,
and a narrow job port backed by [BullMQ](https://docs.bullmq.io/) or
[Temporal](https://docs.temporal.io/develop/typescript) when persistence,
multiple workers, retry state, and operational visibility are required. A
recursive `setTimeout`, `setInterval`, or an ad hoc EventStore table must not
become a job queue.

The concrete dependency is added and exactly pinned only with its first real
use case. Selecting BullMQ or Temporal also accepts the corresponding
Redis/PostgreSQL or Temporal Service operating boundary; the zero-setup local
daemon will not require those services speculatively. Job handlers must be
idempotent, cancellable, and budgeted, and must write domain results to the
EventStore before clients receive a projection.

## Startup order

1. Parse and validate typed process configuration.
2. Derive the workspace scope from stable filesystem identity and effective
   configuration fingerprint.
3. Open protected run/data directories and acquire the daemon lifecycle lease.
4. Open and recover EventStore, then construct the local-model service. Cache
   inspection begins in the background and never starts a download.
5. Start the authenticated gRPC server on a private endpoint.
6. Publish the discovery descriptor last, only after the service is ready.
7. When the verified model is ready, activate its inference lease and construct
   Runtime plus the Pi adapter. A failed activation remains observable and can
   be explicitly retried without downloading the model again.

Any failure before publication remains undiscoverable and is cleaned up in
reverse ownership order. The process never falls back to an embedded Runtime.

## Runtime and recovery boundaries

The local-model adapter streams text into Pi with one active inference and a
fixed 4096-token context. Each user, follow-up, or steering message is limited
to 64 KiB at the daemon command boundary, matching the local-model per-message
ceiling. The Kernel's 256 KiB limit caps the complete constructed inference
input as defense in depth; it is not a single-turn message limit. Inactive
sessions rotate before accepting a new turn at 32 historical turns or 4 MiB of
user text. During an active turn, at most eight follow-ups totaling 512 KiB may
be queued. Budget validation and durable command application share one daemon
FIFO lane, preventing concurrent RPCs from passing checks against stale state.

EventStore remains the durable source of session and execution events. Pi
history hydration is not implemented yet, so daemon restart safely rotates any
session containing a historical turn—even a completed one—instead of silently
resuming with incomplete model context. The old session remains queryable in
EventStore.

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
Focused tests cover composition, activation retry, revisions, rotation, and
recovery. The built production launcher spawned an independent authenticated
daemon, observed `serving`, `setup-required`, `allowedActions=[download]`, and
revision `2`, created no GGUF, and cleaned up its temporary process and
directories. This verifies startup composition only: real-GGUF download,
activation, inference, cancellation, and release packaging remain unverified.
