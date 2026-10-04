# @lyonbrown/local-model

Daemon-owned local-model lifecycle for Lyonbrown. The root API exposes only the pinned model identity, typed status, explicit download/cancel commands, and an opaque runtime lease. It does not accept or reveal model URLs, filesystem paths, or `node-llama-cpp` objects.

## Supply-chain policy

- `node-llama-cpp@3.22.1` is an exact, required runtime dependency. Its exact-version official platform packages provide the prebuilt native capability.
- pnpm keeps the upstream `postinstall` script disabled. The root package also sets `config.nodeLlamaCppPostinstall` to `skip`, protecting installs performed by other package managers even though pnpm is the only supported repository tool.
- Runtime loading always uses `build: "never"`, `skipDownload: true`, and `usePrebuiltBinaries: true`. A missing or incompatible prebuild becomes a typed `native_runtime_unavailable` result; it never downloads llama.cpp sources or compiles on first use.
- Merely creating the service never downloads a GGUF. The pinned model download begins only after `startDownload()` and uses a commit-pinned Hugging Face URL internally.
- `pnpm --filter @lyonbrown/local-model test:prebuild-smoke` is the explicit Windows CPU smoke. It uses `dryRun`, requires the official prebuild, and keeps source builds and downloads disabled.

Model bytes are downloaded to a unique staging directory, SHA-256 verified, then renamed into place. Tests use injected adapters and tiny fixtures; package installation and tests never download the 1.11 GB model.

There is no filesystem-backed default factory. The `./composition` entry requires a `SecureLocalModelCapability`; missing capabilities fail closed. The capability never exposes a URL, path, file handle, or native object. Loading requires a verified native anchor, and the package keeps that anchor until model loading finishes. The native-provider slice must implement the anchor so writes, deletion, reparse-point replacement, and file-identity drift are rejected from verification through load completion; agentd integration remains blocked until that provider exists.

Cache inspection begins in the background, so service construction returns immediately with a typed `checking` status. A provider-owned process lock serializes downloads and activation across daemon processes; a waiter rechecks the verified cache after acquiring the lock. Each concurrent `load()` owns an independent native lease and receives an abort signal. Daemon shutdown must call `dispose()`: it aborts cancellable work, joins tracked tasks up to a bounded timeout, and continues fencing and disposing any lease that arrives after shutdown returns.
