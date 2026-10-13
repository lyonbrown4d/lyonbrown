# System One pre-kernel decision layer

> Status: `open-jev@0.1.2`, daemon pre-kernel composition, protected weight installation, and a
> real Windows model smoke are implemented. System One runs before Pi when the configured model is
> installed and valid. Missing, corrupt, or failed providers persist a safe fallback before Pi.
> The adopted action is currently fixed to `continue_pi`; model classifications cannot change
> authority or routing yet.

## Target architecture

System One is the Lyonbrown capability; OpenJev is the current provider. Shared state and closed
`choice`, `score`, and `noul` questions produce validated probability distributions rather than
arbitrary generated text.

```mermaid
flowchart LR
  Client[TUI client] -->|authenticated gRPC| Daemon[agentd command boundary]
  Daemon --> Guard[deterministic guards]
  Guard --> Decision[DecisionEnginePort]
  OpenJev[OpenJev adapter] -. implements .-> Decision
  Decision --> Policy[daemon routing policy]
  Policy --> Kernel[Pi Agent Kernel]
  Kernel --> Tools[tool policy and approval]
  Decision --> Store[EventStore]
  Kernel --> Store
  Store --> Projection[gRPC projection]
  Projection --> Client
```

The order is deterministic input/authority guards, typed decision, daemon-owned routing policy, Pi
Kernel, then tool policy and approval. A model decision cannot grant authority, bypass approval, or
replace input budgets.

## Implemented boundary

- `@lyonbrown/decision-engine` defines Lyonbrown-owned question, answer, outcome, and lifecycle
  types;
- `open-jev@0.1.2` and `@huggingface/transformers@4.3.0` are exact-pinned;
- provider types stay inside the adapter and cannot enter gRPC, public EventStore schemas, or TUI;
- composition accepts only an already verified absolute local model path and disables Transformers
  remote model access and filesystem caching;
- state, question count, instructions, and options are bounded; answers must remain in the declared
  set, with finite `[0,1]` probabilities and a valid sum; failures become sanitized `undetermined`
  outcomes;
- OpenJev and Transformers load lazily on the first decision in a single-threaded Piscina worker
  with a bounded queue and V8 worker resource limits. Composition reports `configured`, not ready,
  before the model load succeeds. Disposal is idempotent and deadline-bounded, and AbortSignal
  cancellation terminates the corresponding worker instead of leaving an ONNX forward pass on the
  daemon main thread.

## Model and download policy

The initial target is the q4 `kev-0.6b` variant, roughly 340–380 MB. Production composition will not
use OpenJev's Hub downloader. Weights must reuse native secure storage with a pinned revision, file
manifest, SHA-256, byte ceiling, same-filesystem staging, and atomic activation under a separate
decision-model directory. Download is explicit and runs in a separate process, so it never blocks
daemon or ordinary chat startup. The repository provides `pnpm decision-model:install`; it builds
the required artifacts, uses the environment proxy, reports progress, and never writes weights
into the repository or uses OpenJev's implicit Hub cache.

The model root is injected with the dotenv-compatible `LYONBROWN_DECISION_MODEL_ROOT` variable and
must be a fully qualified local path. Windows requires a drive-qualified path and rejects current-
drive roots, UNC paths, and device namespaces. Local development uses an uncommitted `.env.local`
on the `F:` drive; tests inject a dynamically resolved absolute path through dotenv instead of
embedding a drive or user directory in fixtures. A missing or invalid value leaves the provider
explicitly unavailable and never falls back to the home directory or system drive.

## Default-enable gate

- Windows x64 CPU baseline passes offline cache, corruption recovery, concurrency, cancellation
  isolation, and bounded shutdown;
- fixed Chinese and English fixtures report accuracy, calibration error, `undetermined` rate,
  P50/P95, and peak memory;
- question version, provider/model revision, probabilities, threshold, and adopted action are
  persisted before projection;
- built-artifact smoke passes cache reuse, decision, fallback, and cleanup with network and native
  compilation disabled;
- a missing or unverified model follows an explicit recorded Pi fallback while security-sensitive
  paths remain fail closed.

## 2026-10-08 Windows x64 real smoke

The q4 manifest at revision `5a827091b3d58fe4fdfc02a5a5b2ce364047d678` completed a real
download, SHA-256 verification, atomic activation, offline cache reuse, anchored loading, bilingual
inference, and bounded disposal under `F:\Lyonbrown\models\decision`.
The black-box smoke creates the decision service through agentd's public production composition and
connects a real `AgentRuntime`, in-memory EventStore, and observable Pi Kernel instead of invoking
the model adapter directly. `pnpm smoke:real-decision` reported:

- both installation inspection and offline reuse returned `already_installed`;
- first lazy-load inference took about `13707.70 ms`; the next three took `109.96–164.45 ms`, with a
  four-sample P50 of `153.80 ms`;
- observed process peak RSS was `840,572,928` bytes;
- every run persisted `user_message_submitted → decision_recorded → run_started → run_completed`
  before projection and started Pi exactly once;
- all four fixtures returned complete validated answers; the Chinese change-and-test fixture was
  classified as `code-change`, while explanation fixtures still expose classification errors in
  both languages.

The implementation therefore passes the safety and availability smoke, but not yet the larger
bilingual accuracy and calibration gate. Until that benchmark passes, the daemon records the
decision and adopts `continue_pi`; it never uses the classification to relax authority, skip
approval, or select a higher-risk execution path. TUI installation/status projection and an expanded
exploratory bilingual benchmark are implemented in the checkpoint below.

## 2026-10-14 decision panel and exploratory benchmark

`/decision` opens the control panel's Decision section, also available from the command palette.
`Ctrl+P` remains available during chat-model setup. Both languages cover missing weights, background
inspection, install-lock waiting, progress, verification, installation, cancellation, failure and
retry. `/decision install`, `/decision retry` and `/decision cancel` are explicit actions. Closing
the panel leaves installation running in the background.

The daemon owns the controller and a separate Node installation process. The existing download RPCs
carry `ModelPurpose`; chat and decision requests reach separate controllers, and purpose participates
in replay identity. Cancellation is fenced by operation ID and authenticated daemon epoch, and is
disabled during atomic installation. Success requires a completion marker and exit code `0`.
Cancellation grants 15 seconds for cleanup before terminating only the daemon-owned child.

The independent `decision_model` snapshot uses Lyonbrown types and the existing monotonic transport
revision. Verified weights become `configured`, displayed as “Installed · loads on first use”; the
engine still loads lazily. Missing, checking, installing and failed states leave chat available and
persist the safe pre-Pi fallback. Installation activates the existing stable decision port without
restarting chat.

The local `pnpm smoke:real-tui` run returned `REAL_AGENTD_GRPC_TUI_SMOKE_OK`: independent daemon PID
`32976`, authenticated gRPC health `serving`, and real ConPTY navigation from chat setup to the
English decision panel followed by F2 switching to Chinese. CLI exit code was `0`; per-run scratch
was removed and dotenv-injected F-drive weights were retained. The decision smoke also verified
cache reuse through the built installer child process.

`pnpm smoke:real-decision` now runs 18 bilingual pairs, 36 fixtures total: 18 per language and 12
per intent. Human labels treat explanations/translations as low risk, workspace changes as medium,
and production-data deletion, disk formatting and permanent firewall disabling as high. The
benchmark Kernel records start/completion only and does not execute the described actions. Every
fixture verifies `user_message_submitted → decision_recorded → run_started → run_completed`, with
action fixed to `continue_pi`.

| Axis             | Overall accuracy | English       | Chinese       | ECE (10 bins) | Brier  |
| ---------------- | ---------------- | ------------- | ------------- | ------------- | ------ |
| Intent           | 20/36 (55.6%)    | 12/18 (66.7%) | 8/18 (44.4%)  | 0.2503        | 0.6369 |
| Risk             | 14/36 (38.9%)    | 8/18 (44.4%)  | 6/18 (33.3%)  | 0.3318        | 0.7074 |
| Tool requirement | 27/36 (75.0%)    | 13/18 (72.2%) | 14/18 (77.8%) | 0.1198        | 0.3496 |

Fallback rate was `0/36`. First inference took `5070.31 ms`; the remaining 35 had P50 `149.89 ms`
and P95 `243.34 ms`, with observed process peak RSS `897,880,064` bytes. Accuracy counts fallbacks
as incorrect. ECE/Brier are conditional on validated decisions and use each axis's selected-label
probability, not the overall minimum confidence.

These handcrafted examples are exploratory rather than a held-out or release-quality gate. Current
question definitions and classification quality remain inadequate, especially Chinese intent and
risk. Next steps are explicit label definitions, independently expanded hold-out cases, question
version comparisons and calibration; retain advisory behavior until predefined quality gates pass.

## Independent daemon, real gRPC, and TUI black box

`pnpm smoke:real-tui` creates a disposable home under the dotenv-injected
`LYONBROWN_BLACKBOX_ROOT`, starts an independent `agentd` process from built artifacts, reads its
protected discovery descriptor, and performs authenticated loopback gRPC health, handshake, and
snapshot calls. It then starts the real CLI entry through Windows ConPTY instead of using an
in-memory terminal or fake launcher.

The 2026-10-08 local run returned `REAL_AGENTD_GRPC_TUI_SMOKE_OK`:

- the discovery PID matched the independent child process, gRPC health was `serving`, and the
  handshake server epoch and scope matched discovery;
- Pi TUI emitted real ANSI redraws, first rendered `Connecting`, and then rendered authenticated
  gRPC `Ready`;
- the TUI rendered the daemon-snapshot model setup projection and accepted installed-preferred,
  hardware-compatible, or conservative-fallback recommendations without automatically confirming
  or downloading a chat model;
- a real `Q` interaction exited model setup with CLI exit code `0`; the smoke closed the gRPC
  client, stopped the daemon, and removed its per-run F-drive scratch directory;
- discovery polling reopens, reads, and closes the secure directory on each attempt and performs a
  bounded retry for transient Windows `read_descriptor` conflicts during atomic publication.

This covers an independent daemon, the real transport, and real terminal rendering. It does not yet
cover Pi response text, tool approval, or cancellation by itself; the separate real-GGUF scenario
below covers those behaviors.

## Real GGUF flow through an independent agentd

The persistent chat-model root is injected through `LYONBROWN_LOCAL_MODEL_ROOT`. The default remains
workspace scoped, while the local black box uses an uncommitted `.env.local` value of
`F:\Lyonbrown\models\chat`; neither product code nor test fixtures hard-code a drive. Running
`pnpm smoke:real-gguf -- --confirm-download` also requires the explicit
`LYONBROWN_REAL_GGUF_SMOKE=1` gate, preventing ordinary tests from starting network or large-file
work.

The 2026-10-08 process-level run returned `REAL_GGUF_SMOKE_OK`:

- hardware policy selected the pinned-revision, pinned-SHA-256 `Qwen3 0.6B Q8_0` and downloaded
  `639,446,688` bytes to the F drive; the smoke performed a real cancel before retry, verification,
  and atomic activation;
- the harness started a built `agentd` as an independent process and connected through protected
  discovery and authenticated gRPC instead of embedding daemon composition;
- it completed a real short answer, a `write` tool call, explicit `allow-once` approval, exact file
  verification, streaming cancellation of a long answer, and another inference after cancellation;
- after closing the first daemon and verifying the installed digest, it started a new PID/server
  epoch; discovery polling ignored the old descriptor and waited for the new PID and scope before
  cached-model inference;
- it performed bounded shutdown while the second daemon had active inference, removed only the
  per-run F-drive scratch, and retained the persistent verified model;
- Windows native storage now marks the current staging directory delete-on-close inside successful
  activation and performs bounded cleanup of same-model crash residue only after private-ACL,
  non-reparse, and strict-name verification.

This is development-machine evidence for the independent C/S path, not final archive, signing,
installer, or clean-machine release-artifact evidence.
