# Lyonbrown Documentation

<p><a href="../zh/index.html">中文</a></p>

Lyonbrown is a local-first, model-provider-neutral Agent Runtime. The Agent owns its identity, context, capabilities, tools, secrets, and working state; models are replaceable compute providers.

## Documents

- [Local gRPC Client/Server Architecture](architecture/grpc-client-server.md)
- [FluxDI Composition Root](architecture/composition-root.md)
- [TUI Visual and Interaction Baseline](design/tui-interaction-design.md)
- [Node.js / TypeScript + Pi Migration Plan](plans/node-typescript-pi-migration.md)
- [Migration Pause Checkpoint](plans/migration-status.md)
- [Configuration Reference](reference/configuration.md)
- [Local Models and Task Routing](reference/local-models-routing.md)

## Current status

Migration is paused. The target remains one C/S architecture in which `agentd`
owns Runtime state and the TUI is an authenticated presentation client. GUI and
Desktop are out of scope, and there is no embedded fallback.

The integrated branch is not yet an end-to-end runnable product. The Node TUI
and Windows native model-storage changes are preserved on feature branches with
known blockers and have not been merged. See the
[Migration Pause Checkpoint](plans/migration-status.md) for exact commits,
verification evidence, blockers, and the resume order.
