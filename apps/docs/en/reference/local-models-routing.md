# Local Models and Task Routing

## The Node daemon owns llama.cpp by default

Lyonbrown has one product architecture: `agentd` owns the runtime, models, tools, MCP connections, and sessions. The TUI and GUI are authenticated gRPC clients. They do not load GGUF files and there is no embedded Runtime fallback.

The Node implementation uses an exactly pinned `node-llama-cpp` as a required product dependency. Release artifacts use only verified prebuilt native binaries. Runtime options set `build: "never"` and `skipDownload: true`, preventing first launch from silently compiling llama.cpp or fetching a native component. A missing or incompatible binary produces a structured unavailable state.

There is currently one built-in managed preset, and it is the default:

| Field                    | Value                                                              |
| ------------------------ | ------------------------------------------------------------------ |
| Preset                   | `qwen3-1.7b-q4_k_m`                                                |
| File                     | `Qwen3-1.7B-Q4_K_M.gguf`                                           |
| Expected download        | about 1.11 GB                                                      |
| Minimum available memory | 4 GiB                                                              |
| Source revision          | `7fb011e9aee6e4dc7adf8430df9ea8de6a466aa3`                         |
| SHA-256                  | `228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f` |

Both revision and digest are pinned in code; the catalog does not use `main`. A fixed resolve URL avoids depending on `X-Repo-Commit` from Hugging Face URI metadata resolution. Adding or upgrading a preset requires code review, a new digest, memory-threshold validation, and a real-model smoke test. A client cannot provide an arbitrary URL, destination, or digest.

## Non-blocking startup and explicit download

Opening the TUI/GUI or starting the daemon never starts a model download. Cache inspection runs in the background, so the UI can immediately show connection and setup state. A missing model disables model-dependent turn submission, but the user can still inspect state, change configuration, or leave.

Only an explicit user action starts a download. The daemon projects these phases:

`checking → setup-required → waiting-for-lock → downloading → verifying → installing → ready`

Cancellation and failure use `cancelled` and `failed`. Every operation receives a new `operation_id`; a stale ID cannot cancel a newer job. Cancellation is too late once atomic installation starts. The daemon owns the job, so closing one client does not implicitly cancel it. Progress and results use the existing versioned `RuntimeSnapshot`; TUI and GUI do not implement downloaders.

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

`packages/local-model` is completing the managed preset, background inspection/download, cross-process locking, digest validation, atomic activation, and native-load lifecycle. It becomes a releasable default-model path only after its native secure-storage adapter is connected to the `agentd` composition root and passes a real-GGUF test.

Until then, the states and interactions in this document are the accepted product contract, not a claim that the current Node `agentd` entry point can already run local inference. The Rust implementation remains a migration-time behavior oracle, not a compatibility runtime.

## Typed task routing

Task routing lives in the daemon. Its input is typed step context covering capabilities, privacy, budget, and explicit user selection; its output is a finite complexity/model tier rather than a free-form plan. The default Qwen3 routing and tool-call adapter still need to be completed in the Node Kernel/model adapter and verified through deterministic domain events. A routing failure must have an explicit fallback and cannot make the control model a single point of failure.
