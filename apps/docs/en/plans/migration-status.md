# Migration Pause Checkpoint

> Paused on: 2026-10-04
>
> This page records an engineering checkpoint, not a release claim. Migration
> work is paused and no implementation task should resume without an explicit
> decision to continue.

## Integrated on `migration/node-typescript-pi`

- Node.js 24, pnpm, strict ESM, repository gates, and bilingual VitePress.
- TypeScript domain, protocol/gRPC, authenticated client, launch coordinator,
  durable Runtime adapter, Pi kernel adapter, SQLite EventStore, local control
  plane, native Windows path verification, and local-model foundation.
- GUI/Desktop product code and plans have been removed.
- Root Cargo metadata, Rust crates/providers, the Rust `agentd` half, and the
  Rust build script were removed by `2594ad9`.
- Node protobuf sources are the single schema source. A versioned SHA-256
  fixture and reflection assertions replace the removed Rust parity test.
- Workspace identity golden vectors keep their original bytes and expected
  scope IDs under a language-neutral name.

The cleanup specification review is approved. Its code-quality review was
interrupted when this pause was requested and therefore remains open.

## Preserved but not integrated

### Node TUI checkpoint

- Branch: `feature/agent-cli-node`
- Commit: `b71a6b6`
- Replaces `apps/agent-cli` in place with TypeScript and pins
  `@earendil-works/pi-tui@1.0.0`.
- Implements a tested vertical slice for direct chat entry, immediate
  connecting UI, authenticated launcher injection, snapshot streaming and
  revision fencing, English/Simplified Chinese, six themes, model setup,
  approvals, command palette, Activity Inspector, and terminal text safety.
- Fresh pause-time evidence: lint and typecheck passed; 18/18 tests passed.
- The package build did not pass at the checkpoint: the repeated dependency
  build could not resolve `@lyonbrown/local-control-plane` from
  `@lyonbrown/agent-launch`.
- The production executable still requires a concrete secure launcher and
  intentionally fails with `SECURE_AGENT_LAUNCHER_REQUIRED` without one.
- The change has not completed independent specification or quality review.

Until this branch is corrected and merged, the migration branch still contains
the old `apps/agent-cli` Rust source as an inert, non-workspace snapshot. Root
Cargo metadata has already been removed, so it is not a supported build path.

### Windows native model storage checkpoint

- Branch: `feature/native-model-storage`
- Commit: `e389879`
- Contains the Windows DACL, reparse-point, file-identity, install-lock,
  staging, size-bound, atomic-activation, and installed-file anchoring work.
- Its package suite reported 84 passing tests and coverage above the package
  statement/branch thresholds.
- It is blocked and must not be merged: the real `node-llama-cpp` downloader
  writes `filename.ipull` and renames it to the final filename, while the
  current staging implementation pre-opens that final filename without delete
  sharing. The rename fails with `EPERM`, and cleanup can leave the staging
  directory behind.

## Current product state

The migration branch is not an end-to-end runnable product checkpoint. A real
production `agent` still needs:

- a reviewed Node TUI merge;
- concrete native discovery, process-spawn, and authenticated client adapters;
- the production `agentd` composition root and bounded shutdown wiring;
- a corrected native model-storage/download contract;
- complete tools, MCP, approvals, context, secrets, packaging, and release
  verification.

No embedded Runtime, GUI fallback, or Rust compatibility runtime should be
introduced to bridge these gaps.

## Resume order

1. Fix and review `feature/native-model-storage` using a real `.ipull` rename
   integration test and verified cleanup.
2. Fix the TUI dependency-build failure, add concrete launcher composition,
   then complete specification and quality review of `feature/agent-cli-node`.
3. Merge the Node TUI, which removes the final inert Rust source under
   `apps/agent-cli`.
4. Complete `agentd` production composition and run an authenticated local
   daemon/TUI end-to-end test.
5. Run frozen install plus full format, lint, typecheck, test, build, docs, and
   `ci-gate` verification before making a runnable or release claim.
