---
title: "Lyonbrown Documentation"
---

<p><a href="/zh/">中文</a></p>

Lyonbrown is a local-first, model-provider-neutral Agent Runtime built on Node.js / TypeScript. The Agent owns its identity, context, capabilities, tools, secrets, and working state; models are replaceable compute providers.

## Documents

- [System Architecture Overview](/en/architecture/overview/): component responsibilities, Root/Subagents, cross-node tasks, and the optional Center.
- [Local gRPC Client/Server Architecture](/en/architecture/grpc-client-server/)
- [Composition Root](/en/architecture/composition-root/)
- [Electron Desktop](/en/architecture/electron-desktop/)
- [System One Decision Layer](/en/architecture/system-one-decision-layer/)
- [TUI Visual and Interaction Baseline](/en/design/tui-interaction-design/)
- [TUI Local Real-Scenario Black-Box Validation](/en/design/tui-black-box-validation/)
- [Iteration Roadmap](/en/plans/roadmap/)
- [Configuration Reference](/en/reference/configuration/)
- [Local Models and Task Routing](/en/reference/local-models-routing/)

## How it runs

The TUI and Electron Desktop connect to `agentd` over authenticated gRPC. The daemon owns Runtime, tools, models, and EventStore; clients handle input, approvals, and state display.

Operations target Windows, macOS, and Linux. See [Iteration Roadmap](/en/plans/roadmap/) for delivery order and acceptance records covering cross-platform operations, single-node tasks, local multi-agent execution, cross-node collaboration, and optional Center services.
