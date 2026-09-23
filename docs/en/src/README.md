# Lyonbrown Documentation

<p><a href="../zh/index.html">中文</a></p>

Lyonbrown is a local-first, model-provider-neutral Agent Runtime. The Agent owns its identity, context, capabilities, tools, secrets, and working state; models are replaceable compute providers.

## Documents

- [Architecture: Local-First Agent Runtime](architecture/local-first-agent-runtime.md)
- [FluxDI Composition Root](architecture/composition-root.md)
- [Phase-One MVP Implementation Plan](plans/mvp-implementation-plan.md)
- [Configuration Reference](reference/configuration.md)
- [Local Models and Task Routing](reference/local-models-routing.md)

## Current status

- Complete: M0–M9 MVP implementation and local acceptance, including secrets, remote providers and fallback, MCP, SQLite session resume, and embedded llama.cpp enabled by default.
- `agent .` provides the Ratatui/Crossterm UI; `agent ask`, `agent chat`, `agent sessions`, and `agent resume` provide headless and session entry points.
- Cross-platform process sandboxing remains Phase 2 and is outside this MVP.
