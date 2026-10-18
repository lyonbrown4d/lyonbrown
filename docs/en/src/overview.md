# Overview

Lyonbrown keeps agent execution in the persistent `agentd` process. The terminal client and GPUI desktop access sessions, tasks and events through an authenticated protocol. Clients own input, presentation and connection state. Closing a window does not issue a task cancellation.

This guide is for local users and developers and describes the current Rust workspace. Start with [Run locally](quickstart.md), or read [Sessions, tasks and recovery](persistence.md) for execution and recovery semantics. Source behavior described here does not establish runtime acceptance for real models, native SDKs or cross-device operation.

## Workspace

| Directory | Responsibility |
| --- | --- |
| `apps/agentd` | Daemon entry point and service assembly |
| `apps/agent-cli` | Ratatui terminal workbench |
| `apps/agent-desktop` | Native GPUI desktop |
| `crates/agent-browser-runtime` | CEF execution environment embedded in CLI/Desktop with an authenticated reverse channel |
| `apps/agent-control` | Control-plane entry point |
| `crates/agent-client` | Shared authenticated client connection |
| `crates/agent-protocol`, `proto/agent/v1` | Versioned protocol and conversions |
| `crates/agent-runtime` | `rpi-harness` adapter and session recovery |
| `crates/agent-storage` | SQLite state, events and outbox |

Other capability boundaries are described in [Protocol and platform capabilities](protocol.md). A directory or protocol message alone does not establish a working platform capability.

## Explore diagrams

Diagrams are drawn with Mermaid. Click a diagram to open it, scroll to zoom, drag to pan and press `Esc` to close. Each diagram includes its source and an SVG download. Source remains readable when scripts are disabled. The book uses mdBook's native theme selector; diagrams update with the selected theme.
