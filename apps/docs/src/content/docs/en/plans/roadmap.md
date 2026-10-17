---
title: "Iteration Roadmap"
---

> Technical baseline: Node.js 24 LTS, pinned pnpm, ESM, and strict TypeScript.
> This page records usability gaps, verification evidence, and plans as of 2026-10-16; implementation and release acceptance are evaluated separately.

## Current product status

`agentd` owns Runtime, models, tools, MCP connections, and EventStore; the TUI and Electron Desktop are authenticated gRPC clients. Production composition connects Pi Runtime, local models, and durable events. Electron Desktop has connection/chat foundations; full interactions and distribution remain pending. See [Local gRPC Client/Server](/en/architecture/grpc-client-server/), [Composition Root](/en/architecture/composition-root/), and [Electron Desktop](/en/architecture/electron-desktop/) for ownership and capability boundaries.

The Windows development machine has independent-daemon, real GGUF download/activation/inference, tool approval, cancellation, and cache-restart records. macOS arm64 passed independent-daemon and relocatable development-artifact acceptance. See [TUI Black-Box Validation](/en/design/tui-black-box-validation/) and [Release Artifacts](/en/reference/release-artifacts/). These records do not yet establish everyday code-task usability across three platforms or release readiness.

The 36 bilingual exploratory decision-model cases on 2026-10-14 reached 55.6% intent, 38.9% risk, and 75.0% tool-need accuracy. The [System One decision layer](/en/architecture/system-one-decision-layer/) remains fixed to `continue_pi`.

## Single-machine usability gaps (code inspection, 2026-10-16)

The immediate engineering priority is upgrading operations across Windows, macOS,
and Linux by capability. Define shared contracts, reuse portable implementations,
add necessary platform adapters, and verify behavior separately on all three
platforms. Root `AGENTS.md` owns the engineering constraints. Existing Windows
implementations and evidence are the cross-platform baseline. POSIX foundations and a Mac independent-daemon smoke are now implemented; complete three-platform task acceptance remains pending.

Near-term Coding usability means submitting a real repository task through TUI
or Desktop, locating code, editing files, running builds/tests, repairing failures,
and reporting verification results. Multi-turn interaction, cancellation,
reconnection, and post-restart state must be understandable. Chat and a single
file-write smoke do not establish this acceptance level.

| Priority                          | Current evidence and gap                                                                                                                                                                                                                                                      | Next acceptance                                                                                                                                                                                           |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0: cross-platform operations     | Implementation includes POSIX native directories, locks, GGUF/bundle storage and retains Windows adapters. Independent Mac daemon/gRPC smoke passes. CI now covers three platforms; this change has no Windows/Linux runtime results yet.                                     | Complete full gates and real tasks separately on Windows, macOS and Linux, including native dependencies, models, tools, cancellation and restart.                                                        |
| P0: complete a code task          | Registry adds `list_directory`, `search_files` and high-risk approval-gated `process_execute`, which can explicitly invoke Git or project tools. Real argv, workspace bounds and descendant cancellation have tests; model-driven repository task acceptance remains pending. | Verify locate → edit → run checks → repair → report on all three platforms.                                                                                                                               |
| P0: execution-model quality       | Production uses local models. Existing real-GGUF evidence primarily covers answers, prescribed file writes, and lifecycle, without a real repository-task quality benchmark.                                                                                                  | Evaluate understanding, tool selection, repair success, and latency against explicit tasks and acceptance criteria before choosing local models or another Provider path.                                 |
| P0: task context                  | Local inference fixes context at 4096 tokens. Session limits and history hydration exist; application-level runtime pruning/compaction does not.                                                                                                                              | Verify that real multi-turn code tasks retain goals, edits, and verification information as tool output/history grows, with explicit recoverable states at limits.                                        |
| P1: configuration and recovery UX | CLI still passes the fixed `production-default` fingerprint; full effective-configuration normalization/fingerprinting is not connected. Only completed, unambiguous Sessions resume; running, cancelled, pending-approval, and other unsafe states rotate.                   | Isolate daemon scopes correctly after configuration changes and let users find prior sessions, understand interruption outcomes, and continue without replaying uncertain effects.                        |
| P1: real-client acceptance        | TUI has real setup/decision-panel evidence; Desktop has connection/chat foundations. Complete visible real-code-task interactions lack acceptance records.                                                                                                                    | Verify a code task, approval denial/acceptance, cancellation, reconnection, and history inspection through the selected client.                                                                           |
| Before release: delivery          | Relocatable Windows x64 and macOS arm64 development directories are verified. Final archives, signing, installation, upgrade/rollback, clean-machine acceptance, and Desktop distribution remain unfinished.                                                                  | Maintain separate artifacts and acceptance records for all three platforms; revalidate installation, real tasks, cache reuse, and cleanup against each final artifact before claiming a platform release. |

### Cross-platform implementation checkpoint (2026-10-16)

- POSIX uses native `openat`, `O_NOFOLLOW`, `flock`, directory handles and identity revalidation, with extended ACL checks on Darwin. Implementation targets macOS/Linux x64 and arm64; unsupported platforms/architectures fail closed.
- GGUF and decision-model bundles use private directories, complete identity keys, controlled staging, atomic publication and pre/post callback checks. POSIX advisory locks coordinate adapter-managed operations, not hostile same-OS-user processes. Windows native share-deny remains intact. Historical bundle-version collection is still pending.
- File tools add bounded directory enumeration and literal text search. Process execution uses argument arrays, limited inherited environment, bounded output and timeout/cancellation cleanup; high-risk approval is required. It has host-user authority, not an OS sandbox.
- New `pnpm smoke:platform` verifies independent daemon startup, native discovery, authenticated gRPC, SQLite and no model download. This machine returned `PLATFORM_SMOKE_OK darwin-arm64`.
- macOS arm64 passed frozen install, preflight, format, lint, typecheck, test, build and `ci-gate`. The full gates include 46 passing native-adapter tests, 277 agentd tests and 79 script tests; platform-specific skips are excluded from those pass counts. Windows/Linux have no real-run result for this change yet.
- A macOS arm64 development artifact passed independent import, SQLite, platform prebuilt and relocation smoke, including launcher execution under a path with spaces/Chinese characters. Source hard links were detached; final installation/distribution and real GGUF task acceptance remain pending.
- Packaging/launchers now distinguish all three platforms, GGUF/TUI smoke entry points allow them, and decision smoke no longer requires drive F. CI defines three-platform quality gates plus daemon smoke; configuration is not remote passing evidence.

### Cross-platform operation scope

This is implementation and acceptance scope, not completed platform support.
Contracts and observable behavior are shared; permissions, locks, process
termination, terminals, and signing use appropriate operating-system mechanisms.

| Capability                    | Shared behavior                                                                                                   | Platform adaptation and acceptance                                                                                                                                                    |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Files and workspaces          | Read/write/edit, directory listing, search, atomic replacement, and workspace access boundaries                   | Separators, drive letters, case sensitivity, Unicode, symlinks/reparse points, file identity, and access permissions.                                                                 |
| Processes and commands        | Argument arrays, working directory, environment, output, exit results, cancellation, deadlines, and child cleanup | Executable discovery, shell selection/quoting, process-tree termination, and avoiding dependencies on one OS command or shell syntax.                                                 |
| Daemon and local security     | Workspace identity, private directories, discovery, authentication, exclusion locks, startup/shutdown             | Equivalent security constraints using Windows ACLs or POSIX ownership/modes, locking, and stable directory handles.                                                                   |
| Models and durable storage    | Downloads, checksums, install locks, atomic activation, cache, cancellation, and recovery                         | Native prebuilds by OS/architecture, file anchoring, crash cleanup, and separately recorded hardware-backend evidence.                                                                |
| Git and clients               | Git workspace lifecycle, TUI input/output, Desktop connection, and restricted preload                             | Git discovery/path handling, terminal/PTY behavior, client lifecycle, and separately declared desktop-interaction permissions.                                                        |
| Verification and distribution | Shared capability contract tests, real-task scenarios, and check-result formats                                   | Runtime environments and real smoke tests on all three platforms, progressively replacing Windows-only acceptance entry points and verifying artifacts/install lifecycles separately. |

Proceed by capability: files/processes/local security → models/storage → Git/clients
→ real tasks/artifacts. Unsupported capabilities report unavailability and its
reason; missing native security continues to deny operations. Support records must
identify OS, architecture, and runtime evidence; one platform cannot establish
another platform's support.

Add MCP, secrets, and Skills according to real-task needs, retaining default deny
until independently verified. Keep the [System One layer](/en/architecture/system-one-decision-layer/)
on `continue_pi`; independent evaluation and calibration can continue, but its
classification quality is not a prerequisite for ordinary task execution.

## Iteration direction: cross-platform operations → single-machine usability → distributed agents

Distributed architecture was confirmed as the subsequent direction on 2026-10-16.
Advance by acceptance results; candidate models, transport libraries, and automatic
recovery behavior in the proposal are not final selections or implemented features.

1. **Cross-platform operations and real tasks on one machine.** Establish contracts,
   adapters, and acceptance by capability on Windows, macOS, and Linux. Resolve P0
   gaps and validate client, model, tools, context, and event recovery through real
   tasks on each platform. Validate release delivery separately per platform.
2. **Task contracts and local multi-agent execution.** Distinguish Node, Agent,
   Run, Task, and Attempt; define dependencies, authority, budgets, result submission,
   and Root acceptance. Run two local Subagents with isolated sessions/workspaces
   through a replaceable local scheduling interface. Keep Pi in the adapter layer.
3. **Cross-node Subagents.** Add persistent device identity, membership/execution
   authority, secure transport, node capabilities, atomic admission, leases,
   cancellation, and disconnected-result delivery, followed by LAN/VPN candidate
   discovery. Single-machine execution must not require a center.
4. **Distributed Coding, then collection validation.** Use fixed baselines and
   isolated Git workspaces; return commits, logs, and test evidence for Root
   acceptance/integration. Reuse task contracts for structured collection artifacts.
5. **Replication, explicit handoff, and an optional Center.** Preserve one writer
   per Root Session; add incremental synchronization, `ownerEpoch`, and explicit
   coordinator handoff. Do not promise automatic takeover without the protocol
   and corresponding failure tests.

At each delivery checkpoint, run the repository-required frozen install,
preflight, format, lint, typecheck, test, build, and `ci-gate`, with real-scenario
evidence appropriate to the changes. Existing client gRPC and local daemon epochs
remain local boundaries, not a node-to-node protocol or Root authority mechanism.
