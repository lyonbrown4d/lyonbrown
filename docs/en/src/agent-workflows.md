# Plans, context and changes

agentd owns plans, execution modes, context and change records. TUI and Desktop use authenticated commands; presentation state and model suggestions cannot grant tools, models or node access.

This page describes the current code. The default-feature workspace compiles; tests and UI runtime acceptance have not been rerun. Existing screenshots do not validate these interfaces.

## Manage context

In TUI, `/context` shows message count, compactions, revision and estimated input tokens. `/context summary` explicitly reads the summary, `/compact [focus]` requests compaction, and `/context undo` reverses the latest compaction. Desktop exposes corresponding controls on Runtime status.

Compaction reuses Rig Memory's `CompactionPolicy`, `Summarizer`, log and `undo_summary`. The original task stays pinned and system instructions stay in runtime configuration. A summary is reference material and grants no authority. Token estimates use framework heuristics, not provider billing measurements. Detached inspection excludes tool schemas; reserved output tokens are shown separately.

Automatic compaction runs only at a safe boundary before the next model request, with no unfinished tool calls. Manual compaction and undo require no active or uncertain Run, and check the owner epoch and context revision. Summaries use the selected model without tools, with cancellation, timeout and size limits. Summary inputs and outputs always pass through Runtime’s shared privacy boundary. An optional host guard may add restrictions and cannot bypass canonical protection. The intent is durable before provider IO. Unknown outcomes retain the original context and require reconciliation without automatic replay.

Manual compaction first commits a durable maintenance job, releases the global admission lock, then runs in the background. `ContextStatus.maintenance` includes its ID, session, audit Run, revision and state; the original Run is never resumed. Refresh job state after an unknown result instead of automatically resubmitting `/compact`. `GetContextMaintenance` reads job metadata independently of loaded models. `Preparing`, `Sending` and `Uncertain` fence new Runs, mode changes, owner transfers and user filesystem changes in that session. Other sessions can still admit work. In-flight jobs participate in shutdown and upgrade checks. Maintenance jobs use the same canonical privacy preparation and validation as ordinary model IO.

TUI `/context cancel JOB_ID` and Desktop Cancel call `CancelContext` for an exact job and owner epoch. Unsent preparation becomes `Cancelled` after its unsent intent is cleared. An unconfirmed request after sending becomes `Uncertain`. Restart only cleans unsent preparation and never dispatches an existing summary request. Cancellation does not establish that a remote provider ignored the request and does not resolve an unknown outcome.

After inspection, TUI `/context abandon JOB_ID REQUEST_UUID REASON` and Desktop Close uncertain session without replay call `AbandonContext` to retire the old session's uncertain context. It requires no active Run, a stable request UUID, the owner epoch and an explanation. One transaction records the audit, increments `owner_epoch` and marks the job `Abandoned`. The snapshot and sending intent remain intact, and the old session permanently rejects new Runs; continue in a new session. Abandoned jobs no longer prevent upgrades. Abandonment never clears the sending marker, resends a request or converts an unknown result into success.

## Select a session mode

`/mode` shows the mode and tool metadata. Use `/mode read-only`, `/mode plan` or `/mode execute`; Desktop provides the same selection on Runtime status.

| Mode | Restriction |
| --- | --- |
| `read-only` | File, plan, checkpoint and active-memory reads; no file writes, commands, MCP, browser or computer operations |
| `plan` | The read-only set plus plan updates |
| `execute` | The daemon's configured authority, including existing approvals and capability checks |

Modes only narrow existing authority. Switching checks revision and ownership and requires no active or uncertain Run. Run admission stores an immutable mode binding in the same database transaction. Later changes do not alter that binding. This is not an operating system sandbox. Tool metadata describes configuration and mode restrictions; actual execution is further limited by model routing, connected runtimes and the accepted execution plan.

## Track plans

A plan persists steps, dependencies, acceptance criteria and revisions. States are `pending`, `running`, `blocked`, `completed` and `skipped`. Dependencies must form an acyclic graph; a skipped prerequisite does not satisfy a dependency.

Models use `plan_read` and `plan_update` for advisory progress. Model reports, host verification and explicit user completion are separate records. A model cannot create a host verification or user override. Completing a step requires a corresponding durable host command receipt or an explicit user override; the plan never replaces tool authorization.

TUI `/plan` and Desktop Workflow expose inspection and operations. Writes carry request UUIDs and expected revisions. State, events and Outbox commit in one relational transaction.

## Preview, apply and undo changes

`patch_preview` turns unified diffs and expected file revisions into a multi-file change set. `patch_apply` requests approval for an exact change ID, revision and content hash. Intent is durable before filesystem effects, with workspace path and revision checks before writes.

Only UTF-8 text is supported: up to 32 files, 1 MiB per file and 4 MiB total for before/after content and diffs. Existing file permissions are retained; directory metadata is not restored. Multi-file writes are not a filesystem transaction. The host lock serializes this product’s writers only. External editors do not share it, so a race remains between revision checking and replacement; this is not an operating-system compare-and-swap operation. Partial writes, cancellation and interrupted execution become `uncertain` and are never blindly replayed.

`checkpoint_read` inspects a record and `checkpoint_undo` creates a reverse preview. TUI `/changes` lists changes and `/undo` requests a reverse preview. Desktop enables a user apply action only after fetching the exact diff. Undo requires current files to match the original after-images, followed by renewed review and approval. User undo after a completed Run uses a separate host operation without reopening that Run or starting a model request.

`validation_record` references a real persisted daemon command result, never a model-supplied pass flag. Evidence is associated with a historical change revision. It cannot prove that an external editor made no changes during execution or certify the entire current working tree. Unknown command outcomes are not passing evidence.

## Attach images

TUI `/attach "/absolute/path/image.png"` adds a local session attachment; `/detach 1` removes it by index. Desktop uses a native file picker and allows removal before sending. Selecting files does not capture the screen or enable computer control.

`StartRunWithAttachments` admits images with a new Run. Content, hashes and Run acceptance are stored in the same database transaction. PNG, JPEG, WebP and GIF are supported: at most 8 images, 16 MiB per image, 32 MiB total, 8192 pixels per side and 16,777,216 pixels per image. The selected model must also declare image support. Gateway requests additionally obey its body limit: 256 KiB by default, configurable up to 16 MiB, including base64 and message overhead. This may be lower than the attachment limit. Gateway groups restrict image requests to candidates explicitly declaring `vision`.

Clients retain attachments and the same request identity until admission is confirmed. Unknown results do not automatically produce a new request. Attachments cannot be inserted into steering or follow-up input. Image content stays out of ordinary events and debug output but is stored in daemon media and model context. Text privacy protection does not redact images; review image content before submitting it.
