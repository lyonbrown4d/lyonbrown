# Lyonbrown Documentation

<p><a href="../zh/index.html">中文</a></p>

Lyonbrown is a local-first, model-provider-neutral Agent Runtime. The Agent owns its identity, context, capabilities, tools, secrets, and working state; models are replaceable compute providers.

## Documents

- [Architecture: Local-First Agent Runtime](architecture/local-first-agent-runtime.md)
- [Phase-One MVP Implementation Plan](plans/mvp-implementation-plan.md)
- [Configuration Reference](reference/configuration.md)

## Current status

- Complete: M0–M4, including the core, providers, Tool Runtime, profiles, workspaces, skills, and prompt compilation
- Delivered early: the Ratatui/Crossterm `agent .` terminal UI
- Delivered early: a feature-gated in-process llama.cpp provider and CLI configuration path; a real-GGUF smoke run remains an M9 acceptance item
- Next: M5 — Secrets
- The remaining M5–M8 work is pending; cross-platform process sandboxing is Phase 2
