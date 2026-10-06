# Node.js / TypeScript + Pi Delivery Plan

> Status: implementation in progress. The supported product scope is the local
> `agentd` daemon and the `agent` terminal client. GUI/Desktop and the previous
> Rust implementation are not compatibility targets.

## Product boundary

Lyonbrown uses Node.js 24 LTS, pnpm, ESM, and strict TypeScript.

- `apps/agentd` is the only composition root. It owns Runtime, Pi adapters,
  models, tools, MCP connections, secrets, EventStore, and local discovery.
- `apps/agent-cli` is an authenticated gRPC TUI client. It owns presentation
  state only and starts directly in chat when the user runs `agent`.
- `apps/docs` is the bilingual VitePress documentation site.
- There is no embedded Runtime, Rust bridge, GUI client, or dual-write path.

Pi-specific types remain inside adapters and never enter protobuf contracts,
domain events, or UI models. Historical behavior that is still required must
be captured by TypeScript regression tests or language-neutral fixtures.

## Target packages

- `packages/protocol`: protobuf sources, generated gRPC bindings, wire limits,
  and compatibility fixtures.
- `packages/agent-domain`: identifiers, events, invariants, and pure reducers.
- `packages/kernel-pi`: Pi Agent Core and Pi AI adapters.
- `packages/agent-runtime`: persistence-first orchestration and snapshot feeds.
- `packages/agent-client`: authenticated gRPC, reconnect/replay, and epoch fencing.
- `packages/local-control-plane`: discovery, lifecycle, and native access control.
- `packages/control-plane-native`: secure operating-system capabilities.
- `packages/event-store`: SQLite storage, integrity, and recovery.
- `packages/local-model`: managed `node-llama-cpp`, explicit download, verification,
  atomic activation, and model lifecycle.
- `packages/ui-model`: Pi-independent TUI state and command projections.

## Current progress

| Phase          | Status      | Delivered / remaining work                                                                                            |
| -------------- | ----------- | --------------------------------------------------------------------------------------------------------------------- |
| P0 Engineering | Complete    | Node/pnpm baseline, strict ESM, unified gates, bilingual VitePress                                                    |
| P1 Protocol    | In progress | protobuf/gRPC, authenticated client, replay and wire tests exist; expand black-box fixtures                           |
| P2 State       | In progress | reducers, SQLite EventStore, recovery, and Windows path anchoring exist; complete migration/rollback exercises        |
| P3 Runtime     | In progress | Pi kernel and durable Runtime adapters exist; finish tools, MCP, approval, context, and secret paths                  |
| P4 Daemon      | In progress | server, control plane, launcher, and local-model foundation exist; finish production composition and process adapters |
| P5 TUI         | In progress | migrate `apps/agent-cli` to Pi TUI, connect the real client, and complete bilingual interaction surfaces              |
| P6 Release     | In progress | relocatable Windows development artifact and native smoke pass; archive, signing, install, upgrade/rollback remain    |

## TUI milestone

The current product milestone is complete when:

- `agent` immediately renders chat without a `chat` subcommand;
- daemon discovery, launch, authentication, reconnect, and stale-epoch handling
  are observable without blocking initial rendering;
- missing models never force a download; the user explicitly starts, cancels,
  or retries a daemon-owned background job with visible progress;
- streaming conversation, cancel, steering, follow-up queue, approvals, command
  palette, activity inspector, model selection, and session recovery work through
  `packages/agent-client` and `packages/ui-model`;
- English and Simplified Chinese, six built-in themes, narrow terminals, SSH/tmux,
  terminal-safe text, and keyboard-only navigation are covered by tests;
- the TUI never reads secrets, model files, EventStore files, or raw daemon tokens.

The accepted layouts, commands, shortcuts, themes, and detailed state behavior
are defined in [TUI Visual and Interaction Baseline](../design/tui-interaction-design.md).

## Definition of done

- a clean checkout passes frozen installation plus format, lint, typecheck, test,
  build, and documentation gates;
- protocol fixtures, recovery tests, and end-to-end daemon/TUI scenarios cover
  cancellation, approvals, reconnect, epoch changes, and hostile payloads;
- EventStore and model storage pass crash, race, DACL, reparse-point, integrity,
  and rollback tests on supported platforms;
- Windows x64/arm64 packaging verifies native addons, daemon/TUI co-installation,
  signing, upgrade, rollback, and uninstall;
- operational diagnostics and recovery documentation contain no secret-bearing
  paths, tokens, prompts, or model-provider credentials.

## Primary risks

- `node-llama-cpp` ABI, prebuild, GPU, driver, and CPU fallback compatibility;
- Windows DACL, file identity, reparse-point, named-pipe, and atomic activation safety;
- terminal capability differences across Windows Terminal, SSH, tmux, and narrow sizes;
- semantic drift in Pi event timing, cancellation, tool calls, and context budgeting;
- EventStore schema mistakes that make local history unrecoverable.
