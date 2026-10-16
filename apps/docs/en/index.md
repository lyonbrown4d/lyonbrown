# Lyonbrown Documentation

<p><a href="../zh/index.html">中文</a></p>

Lyonbrown is a local-first, model-provider-neutral Agent Runtime. The Agent owns its identity, context, capabilities, tools, secrets, and working state; models are replaceable compute providers.

## Documents

- [Local gRPC Client/Server Architecture](architecture/grpc-client-server.md)
- [Composition Root](architecture/composition-root.md)
- [Electron Desktop](architecture/electron-desktop.md)
- [System One Decision Layer](architecture/system-one-decision-layer.md)
- [TUI Visual and Interaction Baseline](design/tui-interaction-design.md)
- [TUI Local Real-Scenario Black-Box Validation](design/tui-black-box-validation.md)
- [Node.js / TypeScript + Pi Migration Plan](plans/node-typescript-pi-migration.md)
- [Migration Progress and Iteration Roadmap](plans/migration-status.md)
- [Configuration Reference](reference/configuration.md)
- [Local Models and Task Routing](reference/local-models-routing.md)

## Current status

The repository is Node.js/TypeScript-only. The TUI and new Electron Desktop
client foundation reuse authenticated gRPC and `agentd`, which owns Runtime,
tools, models, and EventStore.

Independent daemon and real-model flows have verification records on the Windows
development machine. They do not yet establish everyday code-task usability or
release readiness. The current workspace adds POSIX native capabilities, search/process tools and a passing Mac independent-daemon smoke. Complete three-platform tasks, model quality and context management still require validation or implementation. First upgrade operations
across Windows, macOS, and Linux and establish real single-machine task usability,
then local multi-agent execution, cross-node
execution, and an optional center. See [Migration Progress and Iteration Roadmap](plans/migration-status.md)
for evidence and acceptance order.
