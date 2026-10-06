# Local Models and Task Routing

> Verified implementation checkpoint: 2026-10-06. The composition and bounded
> inference path are covered by focused tests, and a production-composition
> real-GGUF smoke passed on the current Windows development machine. Release
> packaging and the final release artifact remain unverified.

## The Node daemon owns llama.cpp by default

Lyonbrown has one product architecture: `agentd` owns the runtime, models, tools, MCP connections, and sessions. The TUI is an authenticated gRPC client. It does not load GGUF files and there is no embedded Runtime fallback.

The Node implementation uses an exactly pinned `node-llama-cpp` as a required product dependency. Release artifacts use only verified prebuilt native binaries. Runtime options set `build: "never"` and `skipDownload: true`, preventing first launch from silently compiling llama.cpp or fetching a native component. A missing or incompatible binary produces a structured unavailable state.

There is currently one built-in managed preset, and it is the default:

| Field                    | Value                                                              |
| ------------------------ | ------------------------------------------------------------------ |
| Preset                   | `qwen3-1.7b-q4_k_m`                                                |
| File                     | `Qwen3-1.7B-Q4_K_M.gguf`                                           |
| Fixed artifact size      | 1,107,408,544 bytes                                                |
| Minimum available memory | 4 GiB                                                              |
| Source revision          | `7fb011e9aee6e4dc7adf8430df9ea8de6a466aa3`                         |
| SHA-256                  | `228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f` |

Both revision and digest are pinned in code; the catalog does not use `main`. A fixed resolve URL avoids depending on `X-Repo-Commit` from Hugging Face URI metadata resolution. Adding or upgrading a preset requires code review, a new digest, memory-threshold validation, and a real-model smoke test. A client cannot provide an arbitrary URL, destination, or digest.

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

## Current implementation status

`packages/local-model` implements the managed preset, background
inspection/download, cross-process locking, digest validation, atomic
activation, native loading, and a Lyonbrown-owned streaming inference lease.
The Windows secure-storage adapter and service are connected to the production
`agentd` composition root; the TUI remains only an authenticated client.

The default small-model lease admits exactly one active inference. A second
request fails immediately with `resource_busy`, never queues, and never creates
another context. Accepted requests use one sequence in a fixed 4096-token
context, propagate cancellation, and enforce bounded messages, aggregate input,
output tokens, output bytes, event queue, and shutdown cleanup. Native errors
are sanitized, and tool-shaped messages or function-call authority fail closed.
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
bypass these limits. After daemon restart, any session with a historical turn
is safely rotated because Pi history hydration is not implemented yet. Durable
events remain in EventStore.

The production-composition real-GGUF smoke on the current Windows development
machine verified the pinned `Qwen3-1.7B-Q4_K_M.gguf` (1,107,408,544 bytes,
SHA-256
`228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f`). Startup
did not download automatically. An explicit first operation was cancelled after
progress, and a new operation completed download, digest verification, and
activation. The smoke also verified a client-visible short answer and usage,
continuation after cancelling a long turn, restart on the same cache without a
redownload and with a new session/empty turns, and active-inference shutdown
within the 15-second bound. It ended with `REAL_GGUF_SMOKE_OK` and removed its
temporary model cache.

This is evidence for the current Windows development machine and production
composition only. It is not verification of release packaging or the final
release artifact. GUI, Desktop, mobile, and MCP remain outside this phase.

## Typed task routing

Task routing lives in the daemon. Its input is typed step context covering capabilities, privacy, budget, and explicit user selection; its output is a finite complexity/model tier rather than a free-form plan. The default Qwen3 routing and tool-call adapter still need to be completed in the Node Kernel/model adapter and verified through deterministic domain events. A routing failure must have an explicit fallback and cannot make the control model a single point of failure.

The current production kernel has an empty tool registry, default-deny
approvals, a rejecting secret resolver, and no MCP connections. Model text is
untrusted output and does not gain tool authority. GUI, Desktop, and mobile
clients remain outside the current iteration.
