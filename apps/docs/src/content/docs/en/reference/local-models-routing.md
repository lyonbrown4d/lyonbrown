---
title: "Local Models and Task Routing"
---

## The Node daemon owns llama.cpp by default

Lyonbrown has one product architecture: `agentd` owns the runtime, models, tools, MCP connections, and sessions. The TUI is an authenticated gRPC client. It does not load GGUF files and there is no embedded Runtime fallback.

The Node implementation uses an exactly pinned `node-llama-cpp` as a required product dependency. Release artifacts use only verified prebuilt native binaries. Runtime options set `build: "never"` and `skipDownload: true`, preventing first launch from silently compiling llama.cpp or fetching a native component. A missing or incompatible binary produces a structured unavailable state.

The daemon carries a bounded trusted candidate catalog rather than one
hard-coded default model:

| Preset              | Quant  | Fixed artifact size | Conservative memory floor | License    |
| ------------------- | ------ | ------------------- | ------------------------- | ---------- |
| `qwen3-0.6b-q8_0`   | Q8_0   | 639,446,688 B       | 2 GiB                     | Apache-2.0 |
| `qwen3-1.7b-q4_k_m` | Q4_K_M | 1,107,408,544 B     | 4 GiB                     | Apache-2.0 |
| `qwen3-4b-q4_k_m`   | Q4_K_M | 2,497,280,256 B     | 8 GiB                     | Apache-2.0 |

Repository, filename, revision, SHA-256, byte ceiling, minimum context, and
license are pinned in code for every candidate; the catalog does not use
`main`, and a client cannot supply a URL, destination, or digest. Adding or
upgrading a candidate requires code review, digest and license validation,
hardware evaluation, and a real-GGUF smoke test.

## Hardware-aware recommendation

Recommendation is neither a new model downloader nor a hand-maintained table
of GPU product names. In the background, the daemon uses `node-llama-cpp` to
read local RAM/VRAM state and trusted GGUF metadata, then uses its
`GgufInsights` configuration resolver to score a fixed 4096-token context.
Concrete `node-llama-cpp` types remain inside the local-model adapter; gRPC and
the TUI receive only Lyonbrown-owned model descriptors and a finite
recommendation reason.

The selection order is deterministic:

1. Prefer a catalog model that is already installed and digest-verified. An
   offline start does not require downloading a different model merely because
   remote evaluation is unavailable.
2. With no installed model, score only trusted candidates inside a fixed size
   budget. The current 1 GiB first-download budget keeps the default small and
   prevents transient free memory from selecting a multi-GiB download.
3. If hardware probing or remote GGUF metadata fails, fall back to the smallest
   trusted catalog candidate.
4. Publish `setup-required` and its allowed actions after recommendation;
   download still requires explicit confirmation in the TUI.

Raw RAM, VRAM, GPU identifiers, and probe failures never enter the client
contract. A model ID or URI outside the trusted catalog cannot participate in
scoring. Recommendation work is bounded and cancellable, and fallback cannot
expand network or filesystem authority.

## Non-blocking startup and explicit download

Opening the TUI or starting the daemon never starts a model download. Cache inspection runs in the background, so the UI can immediately show connection and setup state. A missing model disables model-dependent turn submission, but the user can still inspect state, change configuration, or leave.

Only an explicit user action starts a download. The daemon projects these phases:

`checking → setup-required → waiting-for-lock → downloading → verifying → installing → ready`

Cancellation and failure use `cancelled` and `failed`. Every operation receives a new `operation_id`; a stale ID cannot cancel a newer job. Cancellation is too late once atomic installation starts. The daemon owns the job, so closing the TUI does not implicitly cancel it. Progress and results use the existing versioned `RuntimeSnapshot`; the TUI does not implement a downloader.

Model activation is distinct from download. A verified cached model can enter
`activating`; a failed activation exposes an explicit retry without starting a
new download. The TUI keeps chat disabled during activation even though there
is no download `operation_id`. Model-only updates share the daemon's monotonic
transport revision with Runtime updates.

The downloader uses the Node 24 built-in `fetch` to stream the response body
into native staging. The request, response body, and file write share one
cancellation signal; only transient request failures before response headers
receive finitely bounded retries. Production composition configures Node's
global proxy through `setGlobalProxyFromEnv`, passing only `HTTP_PROXY`,
`HTTPS_PROXY`, `NO_PROXY`, and their lowercase forms. Authenticated loopback
gRPC explicitly disables environment proxy discovery.

## Security and integrity boundary

Production composition must inject a native secure-model storage capability that provides:

- current-user-only directories and files, rejecting junctions, symlinks/reparse points, and identity drift;
- unpredictable same-filesystem staging and immediate cancellation at the byte ceiling;
- fixed SHA-256 validation before atomic activation;
- a stable file anchor from digest verification until `node-llama-cpp` finishes loading, preventing writes, deletion, or replacement;
- a kernel-observable cross-process install lock, followed by cache rechecking;
- cancellation and bounded shutdown for inspection, download, and model-load tasks, including disposal of late native leases.

Production must fail closed when this capability is absent; it must not fall back to ordinary `homedir()` path operations. Model paths, download URLs, digests, native handles, and underlying `node-llama-cpp` objects do not belong in the public gRPC or UI contract.

## Inference and recovery boundaries

`packages/local-model` owns managed presets, background inspection/download, cross-process locking, digest validation, atomic activation, native loading, and a Lyonbrown-owned streaming inference lease. The production `agentd` composition root assembles the Windows secure-storage adapter and service; the TUI is an authenticated client.

The default small-model lease admits exactly one active inference. A second
request fails immediately with `resource_busy`, never queues, and never creates
another context. Accepted requests use one sequence in a fixed 4096-token
context, propagate cancellation, and enforce bounded messages, aggregate input,
output tokens, output bytes, event queue, and shutdown cleanup. Native errors
are sanitized; tool definitions, tool calls, and tool-result history are mapped
and strictly validated at the adapter boundary.
Qwen3 inference always passes `budgets.thoughtTokens=0` to `node-llama-cpp`,
preventing hidden thought from exhausting the client-visible output-token
budget.

The daemon command boundary limits each user, follow-up, or steering message to
64 KiB, matching the local-model per-message ceiling. The production Pi
adapter's 256 KiB limit is a defensive cap on the complete constructed
inference input, not a single-message allowance. Inactive sessions rotate
before the next turn at 32 historical turns or 4 MiB of user text; active-turn
follow-ups are limited to eight entries and 512 KiB in aggregate. Budget checks
and persistence run in one daemon FIFO command lane so concurrent RPCs cannot
bypass these limits. After daemon restart, bounded completed history is rebuilt
from durable domain state and supplied to Pi. Sessions with steering, pending
approval, unresolved work, uncertain tool outcomes, invalid phases, more than
128 reconstructed messages, or more than 256 KiB of history are safely rotated.
Durable events remain in EventStore.

## 2026-10-07 Windows validation record

Focused tests cover composition and the bounded inference path. The production-composition real-GGUF smoke reads the actual hardware selection
and verifies its pinned digest from the same trusted catalog rather than
hard-coding a model in the script. The 2026-10-07 run covered no automatic
download; explicit cancel/retry, progress, digest verification, and activation;
pinned/auto selection; client-visible answer and usage; a real `write` tool call,
approval projection, `allow-once`, and exact file content; turn cancellation;
cached restart with a new session; and shutdown within a 30-second budget. It
ended with `REAL_GGUF_SMOKE_OK` and removed its temporary model cache.

This is evidence for the current Windows development machine and production
composition only. It is not verification of release packaging or the final
release artifact. GUI, Desktop, mobile, and MCP remain outside this phase.

## Typed task routing

Task routing lives in the daemon. Its input is typed step context covering capabilities, privacy, budget, and explicit user selection; its output is a finite complexity/model tier rather than a free-form plan. Qwen3 tool calling is mapped at the adapter boundary; extensions to multi-model routing must follow the same boundaries. A routing failure must have an explicit fallback and cannot make the control model a single point of failure.

The production kernel has workspace-confined `read`, `write`, and `edit` tools
through a native-required `@openclaw/fs-safe` root plus explicit approval. The
first automatic download also fails closed if no trusted model fits the 1 GiB
budget. Its secret resolver rejects resolution, and this composition provides no MCP connection. Model text
is untrusted output and does not gain tool authority. GUI, Desktop, and mobile clients are outside this page's routing scope.
