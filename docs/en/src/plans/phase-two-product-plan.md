# Phase-Two Product and Interaction Plan

> Status: Proposed; intended as the implementation baseline after the M0–M9 MVP
> Scope: model control plane, shared application layer, TUI, GUI, and cross-client interaction contracts
> Depends on: [Local-First Agent Runtime Architecture](../architecture/local-first-agent-runtime.md), [Phase-One MVP Plan](mvp-implementation-plan.md)

## 1. Goals

Phase two is not primarily about adding more providers or UI components. It turns Lyonbrown's architectural differences into a product experience that is visible, explainable, and recoverable:

1. Models are role-based, policy-governed compute. Users can understand, constrain, and switch them without losing Agent state.
2. TUI and GUI share Runtime behavior, event projections, approval semantics, and session semantics instead of implementing separate Agents.
3. Local execution remains the default, while downloads, resource use, remote escalation, and data boundaries remain explicit.
4. Conversation is the primary surface. Model selection, tool execution, approvals, and recovery appear as progressively disclosed activity rather than raw logs.

Phase two is complete only when both TUI and GUI implement the same semantic loop:

```text
Select or automatically configure models
        ↓
Create / resume a Runtime Session
        ↓
Submit work and understand the routing decision
        ↓
Review and decide high-risk tool requests
        ↓
Cancel, fail, fall back, or continue
        ↓
Restart the application and recover the same working state
```

## 2. Current Baseline and Gaps

### 2.1 Existing foundation

- Kernel, state machine, AgentEvent, SQLite resume, and cancellation semantics are implemented.
- The product has one local C/S boundary: `agentd` owns the Kernel and both TUI
  and GUI use the authenticated gRPC contract.
- llama.cpp is enabled by default in `agentd`. A missing GGUF produces
  `ModelSetupRequired`; it does not trigger a startup download.
- The default managed preset is `qwen3-1.7b-q4_k_m`. The daemon supports
  explicit background download, cancellation, and progress on the single
  `RuntimeSnapshot` stream. A missing configured path is not presented as a
  downloadable preset.
- An in-process `agentd` epoch supervisor activates installed models. It keeps
  the old control plane available while building the replacement and publishing
  its endpoint, and rolls back to a retryable state on failure.
- TUI and GUI now complete the shared model-setup loop: explicit download or
  retry, progress, cancellation, retained failures, and English/Simplified
  Chinese presentation over the same Snapshot/RPC contract.
- The TUI selects `zh-CN` for recognized Simplified Chinese system locale tags
  and otherwise falls back to `en-US`. Global `F2` changes client copy from
  setup, approval, or conversation; local `/language` commands never reach the
  daemon or mutate transcript/Session state.
- Client commands bind the accepted server epoch before queueing. Typed
  `EpochMismatch`, `StaleEpoch` mapping, and strict receipt validation fence old
  epoch commands from a replacement Runtime.
- Ollama, Anthropic, and OpenAI remain configurable providers.
- The Scheduler already models Control / Worker / Frontier, Local / Remote, privacy filtering, and bounded fallback.

### 2.2 Main gaps

- `model.provider + model.model` still centers the production path on one primary model rather than a role-based model pool.
- The shared model-setup controls are implemented in both clients. Richer
  disk/resource explanations and the remaining role-based model-management
  surfaces are still incomplete.
- TUI and GUI share remote projections, but the complete product interaction
  loop still needs end-to-end verification.
- The TUI now has a bilingual command palette and discoverable help over one
  exact ten-command catalog. Its bilingual, snapshot-backed, read-only Activity
  Inspector is also implemented. Real session/model pickers remain incomplete,
  and the Inspector is not yet a complete timestamped event trace.
- The complete Theme/Language control palette and locale preference persistence
  remain M12 work even though direct TUI language switching is available.
- The GUI now submits real remote `StartTurn` commands and subscribes to daemon
  state, but the session transcript, streaming, approval, cancellation, and
  resume loop is not complete.
- The current Quality Gate only handles model errors and empty answers.
- Shell execution is still host/unsandboxed, and the UI must never imply otherwise.

## 3. Confirmed Design Decisions for This Phase

### D10. TUI is the reference client; GUI is a graphical peer

The TUI validates new interaction contracts first because it is inexpensive to
test and serves keyboard and SSH workflows. The GUI does not copy its layout,
but both clients must use the same gRPC commands, projections, and approval
semantics. Neither client may access the Kernel directly.

“Reference client” does not make the GUI secondary. New Runtime capabilities are validated in the shared protocol and TUI before entering the GUI so that two clients do not independently guess at daemon behavior.

### D11. Add a UI-neutral application layer

Keep `crates/agent-app` behind `agentd` with a narrow scope:

- Accept typed `AgentCommand` values for session creation, task submission, cancellation, resume, model selection, and approval responses.
- Own Kernel and session lifecycles instead of moving a mutable Kernel between UI tasks.
- Project AgentEvent into stable `RuntimeSnapshot`, `TurnView`, `ActivityView`, and `ApprovalView` values.
- Expose subscriptions and point-in-time snapshots through the gRPC service.
- Contain no Ratatui, GPUI, color, layout, or key-binding knowledge.

`agentd` is the only composition root. TUI and GUI use `agent-client`; there is
no direct or headless Kernel compatibility path.

### D12. Replace “one model” configuration with roles and policy

The fixed model roles are:

- `router`: low-cost, low-latency typed classification with deterministic fallback.
- `worker`: default execution model, preferably local, responsible for tools and primary reasoning.
- `frontier`: policy-enabled remote escalation, not a mandatory step.

Sessions default to `Auto`. Users may pin the session Worker without creating a new session or clearing working memory. Per-message overrides are deferred in phase two to avoid unclear scope and routing explanations.

The role-based schema replaces the old `[model]` schema without a runtime
compatibility mapper. Migration is explicit and configuration errors fail
closed:

```toml
[models]
mode = "auto"

[models.router]
provider = "llamacpp"
model = "auto"
load = "lazy"

[models.worker]
provider = "llamacpp"
model = "auto"
load = "resident"

[models.frontier]
provider = "anthropic"
model = "claude-sonnet"
enabled = false

[routing]
prefer_local = true
allow_remote_fallback = false
max_remote_cost_usd = 1.0
```

### D13. Local-first must not mean silent downloads

When an embedded model is first required, the Runtime returns a structured `ModelSetupRequired`. Clients show:

- model, quantization, source, and license link;
- download size, maximum disk size, and SHA-256 state;
- current RAM, optional GPU backend, and expected execution mode;
- Download, Choose another model, and Exit actions.

An already valid cache may continue without interaction. Download starts only
after an explicit unary `StartModelDownload`; cancellation uses
`CancelModelDownload`. There is no headless `--yes` bypass. Progress and results
use the single versioned `RuntimeSnapshot` stream rather than a second model
event stream.

Managed state carries its source, phase, allowed actions, byte progress,
failure, and `operation_id`. Closing a client does not cancel the daemon job.
Cancel must match the current operation and returns TooLate after `Installing`
begins. The recovery action is Retry, not resume. A client submits only the
built-in preset ID selected by current configuration; URL, digest, and
destination come from the daemon catalog. A missing explicitly configured
`model_path` cannot start a managed download.

The in-process epoch supervisor activates the installed model. The old control
plane stays available while the replacement Runtime is built. The supervisor
starts the new endpoint and atomically publishes its descriptor before stopping
the old endpoint. Address, token, and server UUID rotate while scope, PID, and
the lifetime lock remain stable; clients reconnect and resynchronize
automatically. Build or transition failure keeps the old control plane and
fails closed. Runtime shutdown performs explicit graceful cleanup followed by
abort cleanup for any remaining task.

### D14. Conversation, activity, and inspection are separate information layers

- **Conversation** contains user intent, Agent answers, and content requiring user action.
- **Activity timeline** groups model selection, tools, fallback, failures, and recovery by step and defaults to collapsed.
- **Inspector** exposes raw arguments, tokens, capabilities, context trimming, events, and diagnostics.

Clients must not render every AgentEvent as a chat message or hide approval inside debug logs.

### D15. Approval uses verifiable tool-generated previews

Add a UI-neutral `ToolPreview` beside the Tool trait: target, action, scope of effect, risk, sandbox status, and required SecretRef names. The tool derives this preview from typed arguments; it does not trust free-form model narration.

TUI and GUI expose identical actions:

- Allow once
- Allow this tool for session
- Deny once
- Deny remaining risky actions

Deny is the default focus. Window close, client disconnect, and timeout fail closed. Shell previews show the full command, working directory, environment variable names, and `host/unsandboxed`, never Secret values.

### D16. Retain GPUI until a real vertical slice informs the long-term choice

Phase two keeps the pinned GPUI dependency. The primary risk today is the application contract and end-to-end interaction, not the renderer.

After M13, run a framework gate covering:

- sustainable Windows, macOS, and Linux build and startup;
- Chinese IME, clipboard, keyboard navigation, scaling, and long-conversation performance;
- testable focus and accessibility semantics for critical controls;
- upgrade cost that does not bind business logic to internal GPUI APIs.

If the gate fails, replace only the GUI adapter; `agent-app` and the interaction contract remain intact.

## 4. Model Control Plane

### 4.1 Catalog and lifecycle

Create a unified model inventory with these states:

- `available`: discovered but not yet verified;
- `ready`: credentials, endpoint, or local file verified;
- `downloading`: progress, rate, remaining bytes, and cancellation handle;
- `loading` / `resident`: embedded model loading or in memory;
- `unavailable`: structured reason and recovery action.

CLI commands:

```text
agent models list
agent models status
agent models install <preset>
agent models use <model>
agent models remove <preset>
agent models doctor
```

The GUI Model Center calls the same service and does not implement a second downloader.

### 4.2 Resource policy

Automatic selection evolves beyond startup-time available RAM:

1. Probe CPU, RAM, VRAM, available backends, and disk space.
2. Filter models that cannot run.
3. Choose the largest Worker inside the user's resource limits.
4. Reuse the Worker for routing by default; keep a second control model resident only when benchmarks prove the benefit.
5. Offer downgrade guidance on load failure without silently selecting a remote model.

Only one heavyweight Worker is resident by default. Router inference is lazy or reuses the Worker. LRU unloading may follow later but is not required for M11.

### 4.3 Routing and explanation

Every selection emits a structured `RoutingDecision`:

```text
role + selected model + capability filters + privacy decision
+ estimated cost + fallback eligibility + human-readable reason
```

Default UI shows the current model and a notice when fallback occurs. The Inspector exposes the complete explanation. Remote escalation requires an enabled Provider, policy permission, compatible data classification, and sufficient budget.

### 4.4 Model acceptance

- Routing unit tests use MockProvider with no network or GGUF.
- Every Provider retains protocol-level wire tests.
- llama.cpp retains a small-GGUF smoke test and adds load cancellation, insufficient resources, and corrupted-cache scenarios.
- A fixed task set measures time-to-first-token, total latency, tool-call success, format adherence, and peak memory.
- Quality escalation requires measurable signals; unconstrained model self-scoring is not accepted in phase two.

## 5. Shared Interaction Contract

### 5.1 Visible states

All clients derive the same states from the shared projection:

```text
Initializing
ModelSetupRequired
Ready
Preparing
Streaming
AwaitingApproval
RunningTool
Cancelling
Completed
FailedRecoverable
FailedTerminal
```

States come from AgentEvent and application-command results. Clients do not use timers to guess whether the Kernel is busy.

### 5.2 Common actions

- New session / Resume session
- Submit / Cancel turn / Force stop
- Select Auto or pin a Worker for the session
- Approve or deny a tool request
- Expand activity / inspect details / copy content
- Retry a recoverable operation
- Open settings / help / diagnostics
- Preview and persist theme / switch UI language

Changing models, opening settings, and expanding details do not interrupt a Session. During tool execution or approval, a model change applies to the next step.

### 5.3 Interrupt and exit

- First Ctrl-C cancels the current turn; while approving it acts as Deny once and then cancels.
- Second Ctrl-C force-stops the task while still attempting to flush durable events.
- Escape closes overlays, exits selection, or clears focus; it no longer exits the whole TUI.
- Exit uses `/quit`, the command palette, or an explicit quit shortcut, with confirmation while work is active.

## 6. TUI Design

The [TUI Visual and Interaction Baseline](../design/tui-interaction-design.md) is authoritative for visuals, keyboard behavior, and localization.

### 6.1 Information architecture

Every width preserves one primary conversation flow:

```text
Context line · workspace · mode · model
Conversation + grouped activity
Composer · live steering · queued follow-up
Status line · cancel · details · commands
```

Sessions, Model Picker, Theme, Language, Inspector, and Help use temporary overlays instead of permanent rails. Approval remains inline with task context; only detail views on extremely narrow terminals may temporarily fill the screen. A 30×8 terminal remains operable and panic-free.

### 6.2 Keyboard and commands

- In Ready, Enter submits. While running, Enter steers the current turn and Alt-Enter queues the next step.
- Multiline input uses configurable Ctrl-J; terminals that cannot distinguish Alt-Enter use `/queue <text>`.
- Up/Down scroll conversation when the composer is idle; editing retains cursor priority and PageUp/PageDown scroll.
- First slash commands: `/model`, `/theme`, `/language`, `/sessions`, `/new`, `/resume`, `/status`, `/queue`, `/help`, `/quit`.
- Ctrl-P opens the command palette; `?` opens help when the composer is empty.
- The current executable subset is `/language`, `/queue <text>`, `/help`, and
  `/quit`. Commands without an implementation are visibly disabled; `/queue`
  is also disabled in the palette because it requires composer text. Unknown
  slash commands are rejected locally instead of becoming prompts.
- Approval shows full action labels and shortcuts, with Deny once always focused by default.

### 6.3 Activity presentation

Each step forms one activity group containing model selection, streaming state, tool use, result, and fallback. Default summaries resemble:

```text
● Local worker · qwen3-4b · selected for tool-capable coding task
◆ shell · awaiting approval
↗ Fallback blocked · tool output is confidential
```

Arguments, tokens, timing, and event order appear only when expanded. Tool failures become recoverable activity and model context rather than automatic full-screen errors.

## 7. GUI Design

### 7.1 Scope of the first real version

M13 implements one complete vertical slice:

- Runtime startup and model setup;
- session list, creation, resume, and title;
- streamed conversation;
- activity cards and Runtime Inspector;
- tool approval, cancellation, errors, and retry;
- Model Picker and current routing state.

M13 does not build a code editor, terminal emulator, Git client, or plugin marketplace. Files and diffs begin as read-only previews with Open in External Editor.

### 7.2 Layout

- Left rail: Workspace, Sessions, New Session, filtering, and recovery state.
- Center: Conversation, step-grouped Activity Cards, fixed Composer.
- Right rail: Runtime, Context, Tools, and Diagnostics tabs.
- Below 900 px: rails become drawers; approval uses a modal or side sheet without squeezing conversation.
- Model Center and Settings are full pages, not content packed into the chat inspector.

### 7.3 Interaction and accessibility

- Full keyboard navigation and visible focus with visual and logical order aligned.
- Chinese IME, Unicode graphemes, selection/copy, Markdown code blocks, and virtualized long output.
- Color is never the only status cue; model locality, risk, and errors have text or icon labels.
- Motion communicates state change, respects reduced-motion, and never makes streaming text continuously reflow the whole layout.
- Approval keeps task context visible and focuses the safe action by default.

## 8. Implementation Milestones

### M10 — Shared application layer and interaction contract

Deliver `agent-app` Command/Snapshot/Activity/Approval types, a pure AgentEvent reducer, Kernel task and cancellation/approval channels, TUI migration without regression, and a mock application service replacing static GUI sample state.

Acceptance: identical scripted event traces project to identical TUI/GUI state; resume, cancel, approval, and lagged-event behavior have integration tests.

### M11 — Model control plane

Status: daemon download/cancel, Snapshot progress, integrity verification,
epoch activation, and the shared TUI/GUI model-setup controls are implemented.
Both clients support English and Simplified Chinese for this loop. Commands are
fenced to the accepted server epoch and validate receipts before success is
reported. This completes the shared setup slice, not every M11 deliverable.

Deliver the remaining role-based configuration, model inventory,
doctor/install/status commands, richer hardware/resource explanations, session
pinning, structured RoutingDecision, and old-config migration. TUI and GUI must
continue to use the same Snapshot/RPCs and must not implement a second
downloader.

Acceptance: first run never downloads silently; valid offline cache starts; insufficient resources and remote fallback are explained accurately.

### M12 — Productized TUI

The command-palette/help slice is implemented: one bilingual ten-command
catalog drives slash parsing, `Ctrl-P`, empty-composer `?`, and `/help`, with
30×8, 52×12, and 90×20 coverage. Model Setup and Approval take priority over
overlays; the first approval `Ctrl-C` attempts Deny once and cancellation, and
the second force-stops and exits if the approval remains active.

The Activity Inspector MVP is also implemented. `Ctrl-O` opens a read-only
projection of accepted Snapshot activity with stable turn/step selection,
follow-latest and pinned-target behavior, explicit fresh/stale-cached state, and
Session/daemon-epoch cleanup. English and Chinese layouts use side-by-side panes
at 90×20, stacked panes at 52×12, and one pane at 30×8. Available details include
route/fallback/context/usage evidence, tool arguments/results/duration, outcomes,
and relevant IDs. Revision/epoch gates prevent rejected remote state from
partially updating the view, while caching and bounded field/JSON/collection/
line/character budgets keep rendering work and output finite.

This is not a complete event trace: timestamps, strict event history, historical
`ToolPreview`/capabilities, and complete diagnostics remain absent. Clipboard
copy is not implemented. The next small node is the real model/session selector
work; the broader milestone still includes the Theme picker/overlay and
preference persistence. M12 as a whole is not complete.

Deliver one primary conversation flow; overlays and command palette; session, model, theme, and language pickers; six built-in themes; English and Chinese localization; activity groups; live steering and queueing; inline approval; help; revised interrupt behavior; narrow layouts; and copyable details.

Acceptance: English and Chinese render tests cover 30×8, 52×12, and 90+ widths; six themes cover critical states; Unicode input, steering, queueing, scroll-follow, approval, and cancellation have interaction tests.

### M13 — GUI Runtime vertical slice

The model-setup card, Runtime phase/progress projection, and top-bar
English/Chinese switch are implemented. The next small node is the complete GUI
transcript, streaming, approval, cancellation, and resume loop, together with
the responsive three-column Runtime surface.

Acceptance: a MockProvider desktop end-to-end test and Windows manual smoke pass; no preview-only or fabricated runtime state remains.

### M14 — GUI management and workflow

Deliver Model Center, Settings, diagnostic export, read-only file/diff preview, external-editor handoff, notifications, and recovery entry points.

Acceptance: the full local-model path from not installed to ready requires no command line; failures support retry, cancel, and actionable repair.

### M15 — Advanced routing and reliability

Deliver benchmark-driven dedicated Router decisions, improved Quality Gate, model load/unload policy, cost and latency budgets, routing history, and recovery policy.

Acceptance: a fixed task set proves measurable value before a second model becomes resident; privacy, budget, and tool-observation fallback protections remain covered.

### Parallel safety track — Sandbox and audit

Cross-platform sandbox work continues under the existing design. While host shell remains in use, both clients display that fact. A future backend changes `ToolPreview.sandbox` and execution only, not approval actions or client flow.

## 9. Quality Gates

Every milestone runs:

```text
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets
mdbook build docs/zh
mdbook build docs/en
```

Additional requirements:

- Source files stay below 300 lines and production/test code remains separated.
- UI reducers, routing, and download state machines use deterministic tests.
- Real Provider, GGUF, and desktop smoke tests remain separate from default offline tests.
- Every user-visible failure includes a recovery action, not only a low-level error string.
- TUI and GUI capabilities may ship at different times, but identically named actions retain identical semantics.

## 10. Decision Gates and Deferred Scope

The following choices require evidence from the preceding milestone:

| Decision gate | Trigger | Required evidence |
|---|---|---|
| One product package or separate TUI/GUI bundles | Before the first public installer | Both options must atomically include the exact-version sibling `agentd`; compare install size, upgrade behavior, and release complexity |
| Keep a dedicated Router resident | M15 | Clear latency, memory, or task-success improvement over Worker reuse |
| Long-term GPUI commitment | After M13 | Three platforms, IME, accessibility, performance, and upgrade cost |
| Add per-turn model overrides | After M14 | Frequent real need without damaging routing clarity |
| Concurrent multi-Kernel sessions | After M13 | Defined background-work and resource model |
| Built-in code editor | After M14 | Read-only preview plus external editor cannot satisfy core work |

Explicitly deferred from phase two: Sub-agent orchestration, long-term vector memory, plugin marketplace, remote Sync Server, and a complete IDE. None should block the model control plane or the two-client interaction loop.
