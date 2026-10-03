# @lyonbrown/local-control-plane

Security boundary for discovering and owning the local Lyonbrown daemon. The
package is independent of Pi, gRPC-generated types, SQLite, and all UI code.

## Responsibilities

- resolve an absolute native canonical workspace path together with its
  Rust-compatible filesystem identity, then derive a stable scope ID from that
  identity plus the effective runtime configuration fingerprint;
- create and strictly parse the versioned discovery descriptor shared with the
  Rust behavior oracle;
- generate a new 32-byte bearer token and UUID server epoch for every endpoint;
- delegate descriptor reads, random same-directory temporary publication,
  cleanup, and atomic replacement to an anchored native directory handle;
- reject unexpected fields, duplicate JSON keys, ambiguous endpoints, and
  descriptors above 4 KiB;
- delegate distinct startup and daemon-lifetime leases to OS kernel file
  locks compatible with the Rust oracle;
- bind commands to immutable discovery epochs and fence them both when queued
  and when executed.

The descriptor wire fields remain `schema_version`, `transport`, `address`,
`pid`, `server_instance_id`, `token`, and `scope_fingerprint`. Public TypeScript
objects use camel case and do not expose gRPC types.

## Native security integration is a release gate

The package intentionally ships no path-based Node filesystem fallback and no
`proper-lockfile` fallback. `agentd` must inject a
`PlatformControlPlaneSecurity`; otherwise every platform fails closed with
`NATIVE_SECURITY_PROVIDER_REQUIRED`. Test fakes live only under `tests/` and
are not exported by the package.

Native adapter authors use the controlled
`@lyonbrown/local-control-plane/native-adapter` entry to implement the contract
and create an opaque provider. The root entry does not export secure handles,
constructors, result unwrappers, or parity helpers. Each build removes `dist`
before TypeScript emits so deleted source cannot survive as stale package
output.

Opaque providers, run directories, and discovery clients keep native adapters
and handles only in module-owned `WeakMap` state. Reflection, JSON, inspection,
and structured cloning expose no native capability or raw operation method.
Internal constructors require an unexported identity token before they touch a
handle or register WeakMap state, so `prototype.constructor` cannot mint a
capability around attacker-controlled handles.

The production Node-API adapter is not implemented yet and is an explicit
agentd completion gate. Its Windows implementation must:

1. create protected, current-user-only DACLs with inheritance disabled;
2. verify current-user owner and DACL identity on directories and files;
3. open paths with `FILE_FLAG_OPEN_REPARSE_POINT` and reject every reparse
   point;
4. obtain the same high-resolution volume/file ID as Rust's `file-id` crate;
5. retain an anchored directory handle for every relative read, temporary file,
   atomic replacement, and lock operation.

Its POSIX implementation must retain a verified `dirfd`, traverse and open via
`openat` with `O_NOFOLLOW`, verify owner and mode on the opened object, publish
with same-directory `renameat`, and use `flock`/`fcntl` kernel locks compatible
with Rust `std::fs::File::try_lock`. Bigint `dev`/`ino` may be used only on a
platform proven by parity tests to match Rust. Windows Node `stat` values are
not accepted as substitutes for volume/file ID. Scope framing targets the same
64-bit Rust builds as the shipped daemon, so `usize` lengths are encoded as
eight-byte little-endian values.

Neither adapter may shell out, re-resolve an anchored parent path, weaken
owner-only permissions, or expose tokens/native causes through logs or errors.

## Lease semantics

`agentd.start.lock` elects one first-start client and is released once daemon
readiness is established. `agentd.lock` is held for the complete daemon
lifetime. They are ordinary files locked by the kernel, with ownership bound to
the returned native lock handle. There is no timestamp/PID stale heuristic and
no process-kill behavior. Releasing an old handle must never release a newer
owner's lock. Every lease caches its first release promise. Concurrent and
later callers await that exact promise; a rejection stays cached and is never
converted into false success or an unsafe second native release attempt.

## Verification

```text
pnpm --filter @lyonbrown/local-control-plane lint
pnpm --filter @lyonbrown/local-control-plane typecheck
pnpm --filter @lyonbrown/local-control-plane test
pnpm --filter @lyonbrown/local-control-plane build
```

Coverage gates are 85% for statements, branches, functions, and lines.
