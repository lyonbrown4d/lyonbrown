# @lyonbrown/event-store

Daemon-only, append-only SQLite event log for Lyonbrown. EventStore is the durable source of truth;
clients and Electron renderers must consume daemon projections over gRPC and must never import this
package.

## Contract

- The database is always `<canonical dataDirectory>/lyonbrown.sqlite3`. File-backed stores use WAL,
  `synchronous=FULL`, foreign keys, one immediate write transaction per append, and a bounded busy
  timeout.
- Revisions are signed SQLite 64-bit integers and remain `bigint` throughout JavaScript. Each
  session starts at `1n`; callers provide an expected revision for optimistic concurrency.
- `append` resolves only after SQLite commits. The caller may publish committed events only after
  that promise resolves. The package does not publish events itself.
- Event IDs are globally unique. Reusing one returns `EventAlreadyExistsError`, including an exact
  duplicate; duplicate writes are not silently treated as success.
- Reads are stable by session revision. Session lists are ordered by most recent update and then by
  session ID.
- JSON payloads reject malformed Unicode, proxies, accessors, non-plain prototypes, dangerous keys,
  unsupported JSON values, excessive depth/node counts, payloads over 1 MiB, and batches over 4
  MiB. A `SecretRef` may be stored, but an object combining `secretRef` with a resolved secret value
  is rejected.
- Recovery marks `ToolStarted` without a later terminal event as `outcome-unknown`; it never invents
  a retry.

```ts
import { openEventStore } from "@lyonbrown/event-store";

const store = openEventStore({ dataDirectory: absoluteDataDirectory, pathVerifier });
const committed = await store.append("session-id", 0n, [
  { eventId: "event-id", type: "SessionStarted", payload: {} },
]);
// It is safe to publish `committed` now.
```

## Path security

The production path must be absolute and is canonicalized before the fixed database filename is
joined. The verifier runs before and after opening the database to reject links or path replacement.
Node does not expose enough Windows reparse-point and DACL information to claim that POSIX mode
`0o600` secures a Windows file. Therefore the Windows default fails closed: `agentd` must inject a
platform verifier backed by native reparse-point and ACL checks. Tests may explicitly use
`{ testOnlyMemory: true }` or a test verifier; neither is a production fallback.

SQLite remains the final concurrency boundary. A malicious process that can replace a verified
directory between verification and native `sqlite3_open` is outside the portable Node verifier's
capabilities, so production packaging must grant the daemon data directory only to the current user
and use the platform verifier.

## Native addon and Electron packaging

`better-sqlite3` is a Node-API/native addon. The repository allowlists its install build script and
pins the exact dependency version. CI must test every release target under Node 24.15.x. If the
desktop ships Electron, keep this package in the daemon process and rebuild/package the addon for
the daemon's Node ABI; do not bundle it into the Electron renderer or preload graph. If a future
deployment embeds the daemon inside Electron, use the Electron ABI-specific rebuild as a separate
artifact and test database open, WAL, bigint, migration, and shutdown on each target OS.

The package compiles ESM JavaScript and declarations with `tsc`. Native binaries are not copied into
`dist`; the application packager must preserve `better-sqlite3` as an external runtime dependency
and include its `.node` binary for the target platform and architecture.

## Verification

```text
pnpm --filter @lyonbrown/event-store lint
pnpm --filter @lyonbrown/event-store typecheck
pnpm --filter @lyonbrown/event-store test
pnpm --filter @lyonbrown/event-store build
```

The package coverage gate is 85% for statements, branches, functions, and lines.
