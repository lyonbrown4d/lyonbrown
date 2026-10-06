# @lyonbrown/local-model

Daemon-owned local-model lifecycle for Lyonbrown. The root API exposes only the pinned model identity, typed status, explicit download/cancel commands, and a Lyonbrown-owned inference lease. It does not accept or reveal model URLs, filesystem paths, or `node-llama-cpp` objects. The descriptor includes the fixed SHA-256 and maximum artifact bytes so clients can display supply-chain identity without gaining storage or loading authority.

## Supply-chain policy

- `node-llama-cpp@3.22.1` is an exact, required runtime dependency. Its exact-version official platform packages provide the prebuilt native capability.
- pnpm keeps the upstream `postinstall` script disabled. The root package also sets `config.nodeLlamaCppPostinstall` to `skip`, protecting installs performed by other package managers even though pnpm is the only supported repository tool.
- Runtime loading always uses `build: "never"`, `skipDownload: true`, and `usePrebuiltBinaries: true`. A missing or incompatible prebuild becomes a typed `native_runtime_unavailable` result; it never downloads llama.cpp sources or compiles on first use.
- Merely creating the service never downloads a GGUF. The pinned model download begins only after `startDownload()` and uses a commit-pinned Hugging Face URL internally.
- `pnpm --filter @lyonbrown/local-model test:prebuild-smoke` is the explicit Windows CPU smoke. It uses `dryRun`, requires the official prebuild, and keeps source builds and downloads disabled.

Local-model owns the fixed catalog entry and commit-pinned URL. The current `Qwen3-1.7B-Q4_K_M.gguf` artifact is exactly 1,107,408,544 bytes with SHA-256 `228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f`. The downloader uses the Node 24 built-in `fetch` to stream into native same-filesystem staging, sharing cancellation across the request, response body, pipeline, and file write. It applies finitely bounded retry only to transient failures before response headers, enforces byte ceilings at downloader and manager boundaries, verifies SHA-256, and then invokes `node-llama-cpp`. Tests use injected adapters and tiny fixtures; package installation and ordinary tests never download the model.

The production `agentd` composition retains a Node 24 `setGlobalProxyFromEnv` lease for its lifetime. Only `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`, and their lowercase forms are passed into global proxy configuration; unrelated environment variables are excluded. Authenticated loopback gRPC separately disables environment proxy discovery.

There is no filesystem-backed default factory. The `./composition` entry requires a `NativeModelStorageCapability`; missing capabilities fail closed. It is a narrow privileged filesystem port: it binds operations to model id, revision, SHA-256, filename, and maximum bytes; provides same-filesystem staging and atomic activation; and holds a stable installed-file anchor for the complete local verification-and-load callback. A path is visible only inside that composition-only callback. The root API never exposes a URL, path, file handle, or native object.

The concrete Windows provider rejects writes, deletion, reparse-point replacement, and file-identity drift while an anchor is held, and guarantees same-filesystem atomic replacement of an existing corrupt destination. It is integrated into the production `agentd` composition; there is no ordinary `fs` or home-directory fallback.

Cache inspection begins in the background, so service construction returns immediately with a typed `checking` status. A `proper-lockfile` lock over a native-anchored target serializes downloads and activation across daemon processes, including stale-lock recovery; a waiter rechecks the verified cache after acquiring it. Each concurrent `load()` owns an independent native lease and receives an abort signal. Daemon shutdown must call `dispose()`: it aborts cancellable work, joins tracked tasks up to a bounded timeout, and continues fencing and disposing any lease that arrives after shutdown returns.

## Inference lease

`LocalModelLease.stream()` accepts only Lyonbrown text messages with `system`, `user`, or `assistant` roles. Every request must provide an `AbortSignal`, `maxOutputTokens`, and `maxOutputBytes`. The adapter also enforces fixed message-count, per-message, aggregate-input, token, and output-byte ceilings. Tool messages and native function calls fail closed; model output is emitted only as untrusted text events and is never interpreted as a command or URL.

The default small-model lease permits one active inference. A concurrent request fails immediately with `resource_busy`; it is never queued and never allocates a second context. Each accepted stream creates one sequence in a fixed 4096-token context and passes the stream cancellation signal into native context creation.

Qwen3 inference sets `budgets.thoughtTokens=0` on the `node-llama-cpp` call, preventing hidden thought from consuming the client-visible output-token budget.

The async stream emits `text_delta`, `usage`, and one terminal `done` or sanitized `error` event. A bounded event queue coalesces text under slow consumers, while the request byte ceiling bounds the coalesced text itself. Completion, cancellation, truncation, error, early consumer exit, and lease disposal all abort context creation or generation and release the owned context and sequence. Lease disposal is idempotent and aborts the active inference. If cleanup exceeds the fixed public shutdown bound, every caller receives the same sanitized timeout failure while one background finalizer keeps waiting; the model and llama runtime are released exactly once, and only after the active stream has finished cleanup.

## Real-GGUF smoke evidence

On 2026-10-06, the production composition passed a real-GGUF smoke on the current Windows development machine. The gated test verified no automatic download; an explicitly started first operation cancelled after progress; a new operation completed download, digest verification, and activation; a short client-visible answer included usage; a cancelled long turn did not prevent a subsequent turn; restarting on the same cache did not redownload and produced a new session with empty turns; and shutdown during active inference completed within the 15-second bound. The run ended with `REAL_GGUF_SMOKE_OK` and removed its temporary model cache.

This evidence is limited to the current Windows development machine and production composition. Release packaging and the final release artifact remain unverified. GUI, Desktop, mobile clients, and MCP are outside this phase.
