# @lyonbrown/local-control-plane

Security boundary for discovering and owning the local Lyonbrown daemon. The
package is independent of Pi, gRPC-generated types, SQLite, and all UI code.

## Responsibilities

- derive a stable scope ID from an existing canonical workspace and the
  effective runtime configuration fingerprint;
- create and strictly parse the versioned discovery descriptor shared with the
  Rust behavior oracle;
- generate a new 32-byte bearer token and UUID server epoch for every endpoint;
- publish discovery through a random same-directory temporary file and atomic
  rename, with a 4 KiB read/write limit;
- reject symbolic links, junctions, unexpected fields, duplicate JSON keys,
  ambiguous endpoints, and non-owner-only storage;
- provide separate startup and daemon-lifetime leases through
  `proper-lockfile`;
- bind commands to immutable discovery epochs and fence them both when queued
  and when executed.

The descriptor wire fields remain `schema_version`, `transport`, `address`,
`pid`, `server_instance_id`, `token`, and `scope_fingerprint`. Public TypeScript
objects use camel case and do not expose gRPC types.

## Windows ACL integration is mandatory

Node's `chmod(0o600)` is not a Windows DACL boundary. On Windows,
`openSecureRunDirectory` therefore fails with `WINDOWS_ACL_REQUIRED` unless
`agentd` injects a `WindowsAclProtector`. There is deliberately no permissive
fallback.

The production adapter still needs to be implemented in the `agentd`
composition root as a native Node-API Windows adapter. It must:

1. create protected, current-user-only DACLs with inheritance disabled;
2. verify owner and DACL on directories and files;
3. open paths with `FILE_FLAG_OPEN_REPARSE_POINT` and reject every reparse
   point;
4. convert the supplied Node file descriptor with `_get_osfhandle` and verify
   the already-open descriptor handle, rather than reopening an attacker-
   replaceable path;
5. keep descriptor, temporary file, and lock artifacts inside the protected run
   directory.

Until that adapter is provided, production Windows composition must stop. It
must not shell out to `icacls`, accept a wide ACL, or pass tokens through
arguments, environment variables, logs, or errors.

## Lease semantics

`agentd.start.lock` elects one first-start client and is released once daemon
readiness is established. `agentd.lock` is held for the complete daemon
lifetime. `proper-lockfile` uses atomic directory creation, heartbeat updates,
and a minimum five-second stale threshold. Recovery never calls a process-kill
API and therefore cannot kill a live PID. All participants must use the same
stale/update configuration; manual lock-directory deletion is unsupported and
can compromise exclusivity, as documented by `proper-lockfile`. Its default
compromise handler is intentionally retained so heartbeat or ownership loss
fails closed immediately instead of letting a daemon continue without its
lease.

## Verification

```text
pnpm --filter @lyonbrown/local-control-plane lint
pnpm --filter @lyonbrown/local-control-plane typecheck
pnpm --filter @lyonbrown/local-control-plane test
pnpm --filter @lyonbrown/local-control-plane build
```

Coverage gates are 85% for statements, branches, functions, and lines.
