# Node.js / TypeScript + Pi Migration Plan

> Status: approved migration direction; implementation is in progress. This document does not claim that parity or cutover has been completed.

## Goals and invariants

Lyonbrown will migrate from the current Rust implementation to a Node.js 24 LTS, pnpm, ESM, and strict TypeScript workspace. The migration replaces implementation technology without weakening the local-first product contract.

The non-negotiable invariants are:

- `EventStore` remains the source of truth for sessions, turns, steps, approvals, and recoverable execution state. Pi is the execution engine, not the system of record.
- `apps/agentd` is the only composition root that owns the runtime, models, tools, MCP connections, secrets, and storage. `apps/agent` and `apps/desktop` are authenticated clients and never embed or own a runtime.
- Pi-specific types are contained by adapters. They must not leak into protobuf/gRPC contracts, domain events, or UI models.
- The existing Rust implementation and its tests remain the behavior oracle until TypeScript parity is demonstrated.

## Target architecture

### Applications

- `apps/agentd`: long-lived local daemon and composition root; hosts gRPC, the kernel, Pi adapters, EventStore, tool execution, model setup, and local control-plane services.
- `apps/agent`: terminal client built with Pi TUI (`@earendil-works/pi-tui`) and Lyonbrown UI models; it communicates with `agentd` over gRPC.
- `apps/desktop`: Electron + React desktop client; it uses the same client and UI-model contracts as the TUI and does not host the runtime.
- `apps/docs`: bilingual VitePress documentation.

### Packages

- `packages/protocol`: protobuf sources, generated gRPC bindings, wire DTOs, versioning, and compatibility fixtures.
- `packages/agent-domain`: domain events, identifiers, invariants, and pure state transitions.
- `packages/kernel-pi`: isolated Pi Agent Core and Pi AI adapters.
- `packages/agent-runtime`: persistence-first orchestration, recovery, and the Snapshot feed.
- `packages/agent-client`: authenticated gRPC client, reconnect/replay behavior, and client-side epoch fencing.
- `packages/local-control-plane`: discovery, endpoint publication, process lifecycle, and OS-specific access-control adapters.
- `packages/event-store`: EventStore, migrations, idempotency, and recovery.
- `packages/tooling`: tool/MCP adapters, policy enforcement, approvals, and audit metadata.
- `packages/ui-model`: Pi-independent view models, projections, commands, and interaction contracts shared by TUI and desktop.
- `packages/local-model`: `node-llama-cpp` integration, model cache/setup, download lifecycle, and activation fencing.
- `packages/i18n-theme`: bilingual strings, locale selection, themes, and persisted presentation preferences.

### Technology mapping

- Pi Agent Core (`@earendil-works/pi-agent-core`) executes agent loops behind a kernel adapter.
- Pi AI (`@earendil-works/pi-ai`) provides model-provider integration behind Lyonbrown model ports.
- Pi TUI (`@earendil-works/pi-tui`) supplies terminal rendering primitives for `apps/agent`.
- `node-llama-cpp` supplies the default local inference adapter.
- `@grpc/grpc-js` and generated protobuf bindings preserve the daemon/client boundary.
- Electron + React implement the desktop shell and UI while consuming `packages/agent-client` and `packages/ui-model`.

## Required behavioral parity

The migration must preserve the observable behavior represented by Rust tests and compatibility fixtures:

- protobuf field numbers, optionality, status/error mapping, and forward/backward compatibility;
- runtime and model epochs, fail-closed stale-command rejection, and activation fencing;
- reconnect replay order, cursors, idempotency, snapshots, and event recovery;
- local discovery, authenticated gRPC sessions, authorization scope, and credential rotation;
- tool approval requests, decisions, cancellation, expiry, auditability, and default-deny behavior;
- secret redaction, protected storage, least-privilege access, and no secret persistence in events/logs;
- privacy boundaries for prompts, telemetry, crash data, model downloads, and MCP traffic;
- context assembly, compaction, budgets, stable ordering, and restart continuity;
- skill discovery, validation, precedence, execution, and error reporting;
- MCP connection lifecycle, capability negotiation, tool/resource mapping, cancellation, and recovery;
- explicit model setup state, progress, cancellation, retry, verification, epoch activation, and offline behavior.

## Current progress

| Phase | Status      | Delivered / primary remaining work                                                                                                                             |
| ----- | ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0    | Complete    | Node/pnpm/strict ESM, unified gates, and bilingual VitePress                                                                                                   |
| P1    | In progress | protobuf/gRPC, authenticated client, and replay tests exist; the complete Rust/Node black-box parity harness remains                                           |
| P2    | In progress | domain reducers, SQLite EventStore, recovery, and Windows path anchoring are implemented; real migration/rollback rehearsals remain                            |
| P3    | In progress | Pi Kernel and durable Runtime adapters are implemented; end-to-end tools, MCP, approval, and context migration remain                                          |
| P4    | In progress | gRPC server, control plane, launch coordinator, and local-model foundation exist; daemon composition, system process adapters, and native model storage remain |
| P5    | Not started | TUI/desktop product integration still consumes the design documents and shared UI contract as inputs                                                           |
| P6    | Not started | parity, packaging, signing, upgrade/rollback, and final cutover                                                                                                |

“Implemented” means that a TypeScript slice and its targeted tests exist. It does
not mean that the production entry point is connected or that the Rust oracle
can be removed.

## Phased delivery

### P0 — Engineering foundation and documentation

Establish Node 24.15.0, pnpm 10.33.4, ESM, strict TypeScript, lint/type/test/build gates, VitePress, architecture boundaries, and a frozen lockfile. No runtime behavior moves in this phase.

### P1 — Protocol and parity harness

Create `packages/protocol`, generate deterministic TypeScript gRPC bindings, import golden protobuf fixtures, and build a black-box harness that can run the same scenarios against Rust and TypeScript endpoints. Freeze wire compatibility before migrating runtime behavior.

### P2 — Domain, EventStore, and storage

Port identifiers, events, reducers, snapshots, migrations, replay, and crash recovery into `packages/agent-domain` and `packages/event-store`. Compare event streams and projections against Rust fixtures, including corrupted-tail and duplicate-delivery cases.

### P3 — Kernel and Pi execution adapters

Implement `packages/kernel-pi` and `packages/agent-runtime`; adapt Pi Agent Core and Pi AI without exposing Pi types. Port context, skills, tools, MCP, approval, secret/privacy, cancellation, and error semantics. Validate deterministic domain events for every execution scenario.

### P4 — `agentd`, gRPC, and local models

Build the daemon composition root, authenticated gRPC services, discovery/control plane, and `node-llama-cpp` model setup. Prove epoch fencing, replay, restart recovery, permissions, and Windows service/process behavior.

### P5 — TUI and desktop clients

Build `apps/agent` with Pi TUI and `apps/desktop` with Electron + React. Both clients consume the same protocol, client, UI-model, localization, and theme packages. Prove that neither client owns runtime state and that reconnect/replay produces equivalent UI projections.

### P6 — parity, packaging, and cutover

Run the complete black-box parity suite, migration/recovery tests, long-running soak tests, Windows installers, upgrade/rollback rehearsals, and signed-artifact verification. Only after the definition of done is met may Rust be removed and this migration branch replace the main branch.

## Definition of done

The migration is complete only when all of the following are true:

- all root pnpm gates and every workspace package gate pass from a clean checkout with a frozen lockfile;
- VitePress builds both locales with valid internal links and assets;
- protocol golden tests and black-box Rust/TypeScript parity tests cover every required behavior above with no unexplained divergence;
- EventStore upgrade, replay, crash recovery, idempotency, and rollback rehearsals pass on real migrated data;
- security review covers auth, approval, secrets, privacy, DACLs, MCP/tool execution, dependency audit, and Electron isolation;
- Windows x64/arm64 native-addon, daemon, TUI, desktop, installer, signing, upgrade, and uninstall tests pass on supported environments;
- operational documentation, diagnostics, data export, downgrade policy, and recovery playbooks are complete;
- release approval explicitly records that Rust is no longer required as an oracle.

After—and only after—these criteria and parity are satisfied, remove the Rust implementation in a reviewable cutover change and replace the main branch with this migration branch according to repository governance. Until then, Rust code and tests stay intact.

## Major risks

- **Windows native addons:** `node-llama-cpp` depends on Node ABI, MSVC runtime, GPU backends, architecture, and driver compatibility. Prebuild availability, source-build fallback, checksum verification, and CPU fallback need explicit test matrices.
- **DACL and local transport:** named pipes, discovery files, model/cache paths, and secret material require per-user DACLs. Permissions must fail closed and be revalidated after install, upgrade, service-account changes, and restore.
- **Electron packaging:** ASAR layout, native-module unpacking, rebuilds for Electron's ABI, code signing, SmartScreen/notarization equivalents, auto-update atomicity, and rollback can diverge from development behavior.
- **Semantic drift:** Pi event timing and cancellation behavior may differ from Rust. Domain adapters and golden event traces must absorb those differences without changing public behavior.
- **Data compatibility:** EventStore schema mistakes can make local histories unrecoverable. Forward migrations, backups, rollback boundaries, and fixture-based recovery tests are release blockers.
