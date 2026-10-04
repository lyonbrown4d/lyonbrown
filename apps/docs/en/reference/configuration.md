# Configuration Reference

> Status: the Node/TypeScript configuration loader is not connected to the production entry point yet. This page records accepted boundaries and the target contract; it does not claim that example configuration is currently executable.

## Configuration ownership

Only `agentd` parses and freezes configuration. The TUI may send protocol-defined user choices, but it does not read provider secrets, select storage paths, or construct a separate runtime. The daemon hashes normalized effective configuration into `effective_config_fingerprint`; together with workspace filesystem identity, it determines the local daemon scope.

Discovery, startup and lifetime locks, EventStore, and Runtime within one scope must use the same frozen configuration. A configuration change establishes a new authenticated epoch instead of mutating an old Runtime's security boundary in place.

## Entry-point behavior

`agent` opens the workspace chat TUI by default; `agent .` is the explicit form. The product will not restore `chat`, `ask`, `sessions`, or `resume` compatibility subcommands. The TUI connects to `agentd` over authenticated gRPC. Only the daemon creates the Kernel and owns Runtime state.

Model inspection or download cannot block client startup. An unavailable daemon produces connection state; a missing model produces setup state. Only an explicit user action may start a background model download.

## Accepted precedence and trust rules

The Node loader must use deterministic precedence and complete schema validation and normalization before computing the fingerprint:

1. secure built-in defaults;
2. current-user configuration;
3. an explicitly selected configuration file;
4. allow-listed `LYONBROWN_` environment mappings;
5. explicit CLI overrides.

Workspace configuration is not trusted by default. Provider endpoints, model routing, MCP commands, approval policy, storage paths, and remote-send policy require separate authorization rather than one broad switch. Unknown fields, NULs, relative security roots, paths that cannot be normalized, and conflicting settings fail closed.

The parser, schema library, and file format remain an implementation decision. Until the loader and migration tooling exist, the product promises no legacy multi-format compatibility.

## Default local model

The default provider is daemon-owned llama.cpp and the default preset is `qwen3-1.7b-q4_k_m`. `node-llama-cpp` is a required dependency, but only verified prebuilt native binaries shipped with the artifact are allowed; there is no runtime source-build fallback.

A missing model does not trigger download. The daemon checks its secure cache in the background and projects `ModelSetupRequired`; the user can explicitly start, retry, or cancel a download. URL, digest, and destination come from the daemon's built-in catalog and cannot be configured by a client. See [Local Models and Task Routing](local-models-routing.md).

## Secrets, remote providers, and MCP

Credentials are SecretRef values resolved only when a remote request is sent. They must never enter discovery, logs, Snapshots, the event database, or process arguments. Remote providers are disabled by default. Automatic fallback is limited to explicitly permitted Public data and cannot send Secret or Confidential content or tool results.

The daemon owns MCP server commands and applies a separate trust decision. Workspace-provided executable paths, arguments, and environment need review. Tool calls still use the common risk classification, approval, timeout, cancellation, and output-redaction path.

## Storage and paths

The daemon exclusively owns a SQLite WAL EventStore. Its path derives from workspace identity and configuration scope; clients cannot provide an arbitrary database path. On Windows, a native provider must validate current-user owner/DACL state, reject reparse points, and hold stable handles for the database and WAL/SHM lifecycle. Production startup fails closed without that capability.

An event is accepted only after database commit. During recovery, a tool call that started without a durable outcome is marked outcome unknown and is never replayed automatically.

## Current release blockers

- a versioned Node configuration schema, normalization, and fingerprint implementation;
- separate trust UX for user and workspace configuration;
- SecretRef backends plus log/error redaction verification;
- native secure model-cache storage and stable file anchoring;
- configuration migration, diagnostics, and bilingual error messages.

Until these are complete, the `apps/agentd` production entry point remains fail closed and does not present a hard-coded fingerprint as releasable configuration.
