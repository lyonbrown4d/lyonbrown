# `@lyonbrown/protocol`

Generated TypeScript message, client, and server contracts for
`lyonbrown.agent.v1.AgentService`.

## Contract ownership

During the migration, `proto/agent/v1/*.proto` is an intentionally duplicated
copy of the Rust oracle in `crates/agent-protocol/proto/agent/v1`. Tests compare
normalized source bytes and generated descriptor sets so either copy changing
alone fails immediately. RPC names, package/service names, field tags, enum
numbers, optional presence, and oneof presence are compatibility boundaries.

At P6, after Rust parity and cutover acceptance are complete, the Node proto
directory becomes the single source of truth and the temporary Rust copy can be
removed with the Rust implementation.

## Generation

`pnpm generate` uses the exact, lockfile-pinned `@bufbuild/buf` binary and the
local exact `ts-proto` plugin declared in `buf.gen.yaml`. No remote plugin or
postinstall download participates in generation: the Buf postinstall is
disabled and the platform package is resolved directly from pnpm's integrity-
checked optional dependencies. Generated code is committed. `pnpm
generate:check` regenerates into a system temporary directory and rejects stale
output; it is part of both `test` and `build`.

`pnpm format:check` asks the same locked Buf binary to write formatted output to
a system temporary directory, then compares bytes with Node. The write command
copies that verified output into the source tree. This avoids Buf's Windows
dependency on an external `diff` program when either `--diff` or `--write`
encounters a changed file, including machine-specific Git for Windows
installations. Temporary directories are removed on success and failure.

The generated `uint64` representation is `bigint`, and oneofs are discriminated
unions. Consumers must use generated DTOs only at the transport boundary and
map them to domain or UI view models.

## Security boundary

Local gRPC clients and servers must apply `GRPC_CHANNEL_OPTIONS` and authenticate
every RPC, including health and handshake, with the `authorization` metadata
value produced by `bearerAuthorization`. `authorizeBearer` validates canonical
32-byte base64url tokens and compares decoded token bytes with
`timingSafeEqual`; callers must not log raw metadata or tokens.
