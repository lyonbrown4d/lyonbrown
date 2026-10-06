# Lyonbrown Documentation

<p><a href="../zh/index.html">中文</a></p>

Lyonbrown is a local-first, model-provider-neutral Agent Runtime. The Agent owns its identity, context, capabilities, tools, secrets, and working state; models are replaceable compute providers.

## Documents

- [Local gRPC Client/Server Architecture](architecture/grpc-client-server.md)
- [FluxDI Composition Root](architecture/composition-root.md)
- [System One Decision Layer](architecture/system-one-decision-layer.md)
- [TUI Visual and Interaction Baseline](design/tui-interaction-design.md)
- [Node.js / TypeScript + Pi Migration Plan](plans/node-typescript-pi-migration.md)
- [Migration Progress](plans/migration-status.md)
- [Configuration Reference](reference/configuration.md)
- [Local Models and Task Routing](reference/local-models-routing.md)

## Current status

Migration is active on `main`. The repository is Node.js/TypeScript-only: the
Node TUI and corrected Windows native model storage are integrated, while GUI,
Desktop, embedded Runtime fallback, and Rust compatibility remain out of scope.

The product is not yet an end-to-end runnable checkpoint. Concrete launcher
adapters, the complete `agentd` production composition, and an authenticated
local daemon/TUI black-box flow are next. See
[Migration Progress](plans/migration-status.md) for verification evidence and
the implementation order.
