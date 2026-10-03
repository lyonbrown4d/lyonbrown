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
  duplicate; this check precedes expected-revision validation so retries remain deterministic.
- Exhausting SQLite's signed 64-bit revision range returns `RevisionExhaustedError`. Lock waits use
  the configured busy timeout and return `StorageBusyError` instead of leaking a native error.
- Reads fail closed unless identifiers, canonical timestamps, bigint revisions, row ownership,
  contiguous event revisions, and `sessions.current_revision` all agree. Session lists are ordered
  by most recent update and then by session ID.
- JSON payloads reject malformed Unicode, proxies, accessors, non-plain prototypes, dangerous keys,
  unsupported JSON values, excessive depth/node counts, payloads over 1 MiB, and batches over 4
  MiB. A `SecretRef` has only two accepted closed forms:
  `{ secretRef: "provider:name" }` or
  `{ kind: "secret-ref", reference: "provider:name" }`. Extra fields are rejected recursively.
  Secret-like fields such as `apiKey`, `clientSecret`, `accessToken`, `password`, `privateKey`,
  `plaintext`, and `material` are rejected by default even outside a SecretRef. Domain mappers must
  pass only a redacted logical reference.
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

The production path must be absolute. The verifier first receives the requested path and must inspect
every existing ancestor before `realpath`; this prevents a junction from being hidden by
canonicalization. It then receives both requested and canonical paths. Directory and existing-file
identities are captured before open and compared after open, while reparse-point and ACL checks run
again. The fixed database filename is joined only to the verified canonical directory.
Node does not expose enough Windows reparse-point and DACL information to claim that POSIX mode
`0o600` secures a Windows file. Therefore the Windows default fails closed: `agentd` must inject a
platform verifier backed by native reparse-point and ACL checks. Tests may explicitly use
`{ testOnlyMemory: true }` or a test verifier; neither is a production fallback.

SQLite remains the final concurrency boundary. A malicious process that can replace a verified
directory between verification and native `sqlite3_open` is outside the portable Node verifier's
capabilities, so production packaging must grant the daemon data directory only to the current user
and use the platform verifier.

## Secret boundary trade-off

The secret-field policy deliberately prefers false positives over credential persistence. It uses
case- and separator-insensitive names plus security suffixes such as `*Token` and `*Secret`; it does
not use arbitrary substring matching. Ordinary fields such as `value`, `tokenUsage`, `materialName`,
and `publicKey` remain valid. Add a reviewed domain mapping rather than weakening the codec when a
new event legitimately collides with a denied name.

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
