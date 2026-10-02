# Lyonbrown Documentation

<p><a href="../zh/index.html">中文</a></p>

Lyonbrown is a local-first, model-provider-neutral Agent Runtime. The Agent owns its identity, context, capabilities, tools, secrets, and working state; models are replaceable compute providers.

## Documents

- [Architecture: Local-First Agent Runtime](architecture/local-first-agent-runtime.md)
- [Local gRPC Client/Server Architecture](architecture/grpc-client-server.md)
- [FluxDI Composition Root](architecture/composition-root.md)
- [TUI Visual and Interaction Baseline](design/tui-interaction-design.md)
- [Phase-One MVP Implementation Plan](plans/mvp-implementation-plan.md)
- [Phase-Two Product and Interaction Plan](plans/phase-two-product-plan.md)
- [Configuration Reference](reference/configuration.md)
- [Local Models and Task Routing](reference/local-models-routing.md)

## Current status

- Lyonbrown now has one C/S product architecture: `agentd` is the sole owner of
  the Kernel, models, tools, MCP connections, and session storage; TUI and GUI
  are authenticated gRPC clients with no embedded Runtime compatibility path.
- llama.cpp is a default `agentd` capability. If its configured model is absent,
  the Runtime enters `ModelSetupRequired` without forcing a download. The daemon
  implements explicit background download, cancellation, progress, and epoch
  activation. TUI and GUI now share explicit bilingual setup controls, progress,
  retry/cancel behavior, and fail-closed epoch fencing over the same gRPC state.
- `agent` enters the Ratatui/Crossterm chat TUI by default; `agent .` is the
  explicit equivalent. Legacy chat/headless subcommands are not retained.
- The TUI selects `zh-CN` for recognized Simplified Chinese system locales and
  otherwise uses `en-US`. `F2` switches language globally; `/language`,
  `/language en-US`, and `/language zh-CN` are local client commands and do not
  reach the daemon or modify transcript or Session state.
- The TUI command/help slice is implemented in both locales: `Ctrl-P`,
  empty-composer `?`, and `/help` use one exact ten-command catalog and clearly
  disable unfinished entries. The bilingual, read-only Activity Inspector is
  also implemented over accepted Runtime snapshots: `Ctrl-O` opens stable
  turn/step details, keeps stale cached evidence visible, and adapts across
  30×8, 52×12, and 90×20 terminals. Real session/model selectors and the Theme
  overlay with preference persistence remain M12 work.
- Cross-platform process sandboxing remains Phase 2 and is outside this MVP.
