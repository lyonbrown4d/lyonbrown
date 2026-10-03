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

`pnpm generate` uses the repository-pinned `grpc-tools` protoc and `ts-proto`.
Generated code is committed. `pnpm generate:check` regenerates into a temporary
directory and rejects stale output; it is part of both `test` and `build`.

The generated `uint64` representation is `bigint`, and oneofs are discriminated
unions. Consumers must use generated DTOs only at the transport boundary and
map them to domain or UI view models.

## Security boundary

Local gRPC clients and servers must apply `GRPC_CHANNEL_OPTIONS` and authenticate
every RPC, including health and handshake, with the `authorization` metadata
value produced by `bearerAuthorization`. `authorizeBearer` validates canonical
32-byte base64url tokens and compares decoded token bytes with
`timingSafeEqual`; callers must not log raw metadata or tokens.
