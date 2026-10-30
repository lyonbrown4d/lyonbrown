# Terminal and desktop clients

Both clients access the daemon through `agent-client`. Sessions, run results and approvals come from the service; neither client embeds another agent loop.

## macOS installed entry points

PKGs place each app under `/Library/Application Support/Lyonbrown/Clients/{cli|desktop}/{version}-{full-tree-hash}/` with a separate content-component receipt. Resource or Helper changes select a different directory and component identifier even when the main executable and product version are unchanged. Separate components own `/usr/local/bin/agent` and `/Applications/Lyonbrown.app`; each contains only a symlink to its selected content. The packager verifies the copied tree, disables Installer relocation for every nested bundle, and does not prune old content components or receipts.

The shared launcher and CEF host use the loaded main executable path reported by the macOS kernel to locate the corresponding bundled agentd, framework and Helpers. After the public entry selects another version, an existing process continues to use its original version's companion files. Helpers are copied executables inside the maintained app layout; sandbox entry validates the same library location used by cef-rs before invoking its loader. Package payload checks and OS installation, upgrade and rollback acceptance are separate. See the [local distribution guide](https://github.com/lyonbrown4d/lyonbrown/blob/main/packaging/README.md).

## Terminal workbench

```sh
cargo run -p agent-cli -- --help
cargo run -p agent-cli -- --endpoint http://127.0.0.1:7447 --data-dir /path/to/lyonbrown-data
```

The workbench shows the conversation and composer by default. Sessions, tasks, computer details and help open in an on-demand context panel rather than persistent empty panels. Pending approvals show the tool, arguments and approve/deny controls above the composer. Wide terminals place context beside the conversation; narrow terminals replace the conversation area with context. `Esc` closes the panel. Failed connections display `Disconnected` without simulated model replies. Failed submissions preserve the draft and are not replayed automatically. The matching draft is cleared only after the service confirms acceptance.

| Key | Action |
| --- | --- |
| `F1` / `F2` / `F3` / `F4` / `F5` | Toggle help / sessions / tasks / computer / full approval context |
| `Esc` | Close the context panel |
| `Ctrl+N` | Create a session using the current working directory |
| `Tab` | Select the next session and load its runs |
| `Enter` | Submit input to the selected session |
| `Shift+Enter` / `Alt+Enter` | Insert a newline in the composer |
| Arrow keys, `Home` / `End`, `Delete` | Move the cursor or edit text at the cursor |
| `Ctrl+R` | Reconnect to the daemon |
| `Ctrl+X` | Request cancellation of the active run |
| `Ctrl+Y` / `Ctrl+E` | Approve / deny the first pending approval |
| `PageUp` / `PageDown` | Scroll the open context panel, or the conversation when no panel is open |
| `Ctrl+C` / `Ctrl+D` | Exit the client |

The composer uses `ratatui-textarea` 0.9.3 for cursor editing, multiline text, wrapping and paste. Application shortcuts take precedence over matching editor bindings: `Ctrl+X` still requests run cancellation and `Ctrl+Y` still handles approval. Pending submissions are not sent again; failure preserves the draft, and service acceptance clears the matching draft and its undo history.

The approval area displays the tool and arguments. Compact layouts retain only the pending count and approval shortcuts; use `F5` to read the full arguments. The daemon validates and saves decisions before the UI updates them. Durable runs refresh every two seconds; newly submitted runs also subscribe to an event stream. Exiting restores the terminal and sends no cancellation request.

The UI uses warm dark or warm light solid terminal backgrounds, text and accents. `--theme auto` uses `COLORFGBG` to choose the palette, falling back to dark without a hint. Select `--theme light` or `--theme dark` explicitly when needed. Model replies render Markdown through fixed `tui-markdown` 0.3.10, adapting the theme through its public `StyleSheet` interface. Default features are disabled, omitting code syntax-highlighting dependencies.

The TUI separates presentation through MVVM: `view_model.rs` projects client `App` state into a read-only `ViewModel` containing connection labels, messages, approvals and current context. `view.rs` renders that data and the composer through Ratatui. Input actions and background updates remain in `input.rs`, `backend.rs` and `update.rs`; presentation never dispatches daemon operations.

## Native computer attachment

CLI/TUI and Desktop embed ComputerRuntime in their own processes. Default builds omit native SDKs; explicitly enable the client `native-computer` feature as described in [development and verification](development.md#native-computer-client-builds). This neither enables daemon model tools nor requests OS permission. Target discovery lists window or display metadata under existing permissions, without screenshots or input. A locked or unknown session, or missing screen permission, prevents discovery.

List targets, then explicitly attach using an existing non-nil daemon Session UUID and one target ID:

```sh
cargo run -p agent-cli --features native-computer -- computer targets
cargo run -p agent-cli --features native-computer -- computer attach --session SESSION_UUID --window WINDOW_ID --process-id PID
# For a display, use --display DISPLAY_ID without --window or --process-id
```

`computer targets` does not connect to a daemon. For a window, copy both its ID and `process_id` from that catalog. `computer attach` discovers targets again and requires an exact PID match before retaining the descriptor, authenticating and checking session ownership. A reused window ID cannot silently select a different process. The command stays in the foreground until `Ctrl+C` detaches; process exit also revokes the host connection. Attachment failure reports unavailability without automatically selecting another target.

In the TUI, connect and select a session, then enter local commands in the composer:

```text
/computer targets
/computer attach window WINDOW_ID
/computer attach display DISPLAY_ID
/computer status
/computer detach
```

Use one of the two `attach` forms. Run `/computer targets` first and choose from that catalog. Attachment uses the exact cached descriptor rather than replacing the target by ID. The Computer area displays the originally bound session, scope and connection state; `Alt+PageUp` / `Alt+PageDown` scroll its details. Switching sessions does not transfer the connection; detach before choosing another target or session. Even invalid `/computer` syntax stays local and is never submitted to the model. Reconnection or disconnect revokes the host and clears the catalog, requiring fresh discovery before attachment; actions are not replayed automatically.

See “Computer pane” below for Desktop controls and binding state, and [native desktop execution boundary](protocol.md#native-desktop-execution-boundary) for discovery, permission and fixed window identity limits. Wired entry points do not establish native-control acceptance. macOS checks session identity and lock fields under the documented project policy, rejecting unknown state with `SessionStateUnknown`. Once the backend observes a lock or invalid session, unlocking still requires a fresh explicit attachment.

## Terminal task management

```sh
cargo run -p agent-cli -- task list --session SESSION_ID --page-size 100
cargo run -p agent-cli -- task get TASK_ID
cargo run -p agent-cli -- task reconcile TASK_ID --node-id WORKER_ID --node-endpoint https://worker.example:7448
```

`task dispatch --help` lists dispatch arguments. Supply stable Task and Attempt UUIDs, an idempotency key, and an active parent Run in the same Session. The daemon validates ownership, workspace, deadline and capability scope. `task cancel` requests cancellation; `task accept` requires `--accepted true` or `--accepted false` to decide whether to adopt the Worker result.

Pairing identity and credentials stay in the daemon; an address grants no trust. After a lost submission response, retain the original Task ID and use `get` and `reconcile`, without automatically generating a new task identity. Pages return `next_task_id`; pass it through `--after` to continue.

The TUI `F3` panel queries durable Tasks for the selected session and displays Task and Worker Attempt status separately. Use arrow keys to select a task, `Enter` with an empty composer to inspect it, and `PageDown` to fetch the next page. Reopening the panel returns to the first page. Composer commands `/task ...` use the standalone CLI subcommands and arguments, such as `/task get TASK_ID` or `/task cancel TASK_ID --node-id WORKER_ID --node-endpoint https://worker.example:7448`. Parsing is local; malformed commands never become model prompts.

The TUI saves session drafts, the unbound draft, selected session, context panel and scroll positions. Switching sessions retains independent input. A submission acknowledgement clears only the originating session draft that still matches the submitted text. `/draft list` lists local drafts; `/draft recover SESSION_ID` places the selected text in the composer, and `/draft recover unbound` retrieves the unbound draft, for inspection before the user decides whether to submit it. Drafts whose original sessions are unavailable remain stored and are never automatically attached to another session.

## GPUI desktop

```sh
cargo run -p agent-desktop
```

The desktop uses GPUI and `gpui-kit` for a session sidebar grouped by workspace, multiple chat tabs, conversation input and Browser, Computer, Files, Editor, Diff and Tasks dock panels. The custom title bar reuses the library's `TitleBar`; macOS uses native close, minimize and zoom controls. The session sidebar starts open and the right workbench starts closed. Title bar buttons toggle each side independently and remain available to restore either panel when both are hidden. Collapsing the left sidebar leaves a compact icon rail. Assistant replies use flat prose, while the composer and workspace information occupy separate layers. Light and dark views share colors and dimensions from `theme.rs`.

Desktop presentation follows MVVM: `DesktopViewModel` holds the active session selection, runs, event cursor, messages, approvals and status, and groups sessions by the service-provided `Session.workspace`. It holds no network client, async runtime or connection credentials. `operations.rs` issues requests through the client port, and `projection.rs` applies replies to presentation state. GPUI views read that state and reuse `gpui-kit` buttons, inputs, Markdown, docking and resizable components.

Background results use bounded `async-channel` 2.5.0 queues; producers await capacity when a queue is full. GPUI applies results on its main thread and discards replies for obsolete tabs or generations. Browser frames retain an independent 160 ms refresh timer, and Computer hosts retain an independent 80 ms health timer. Closing a window cleans up local subscriptions and hosts; daemon jobs retain their independent lifetime.

The ellipsis menu in the workbench tab bar lists all six panels: Browser, Files, Editor, Diff, Tasks and Computer. Use it to switch panels when a narrow window hides a tab. The component library still does not expose proper tab names in its accessibility tree; the menu does not resolve that tab accessibility limitation.

Tasks reads durable daemon Tasks and provides first/next pages, details, reconciliation, cancellation and explicit acceptance/rejection. Dispatch requires a selected session with an active Run, a paired Worker node ID, its HTTPS endpoint and instructions. Workspace access defaults to read-only; the read/write choice still requires approval for writes, and the daemon checks paired capabilities. Desktop generates the Task UUID, Attempt UUID and idempotency key before sending, with a one-hour deadline. Unknown outcomes retain the original identity for inspection and reconciliation without automatic resubmission. A successful Worker Attempt still awaits Master acceptance.

### Settings and connection

The title bar's Settings button opens a dedicated page. The daemon address and access token are edited only there; Workspace path selects the daemon-side folder for new chats in the current tab. The endpoint, token and workspace must match the daemon. A workspace for a remote daemon refers to that daemon's filesystem. Appearance offers manual Light and Dark choices. Back to chats and `Esc` return to the chat while preserving panel visibility and chat drafts.

Connect is unavailable while a run is active, a request is pending, or file edits are unsaved. Saving unchanged text also blocks reconnection until that save request finishes. The unsaved-edit check covers both the current Editor and cached file drafts for other sessions. While a connection request is pending, the old Editor remains read-only to prevent writes through the connection being replaced.

### Chat tabs and drafts

Use the sidebar’s “新对话” (New chat) control or the tab bar’s “＋” to create a chat tab. Selecting a session in the sidebar opens its tab or activates the existing tab for that session. Each tab has its own identity and retains its native composer, workspace path, input draft, messages, approvals and run state. Inactive tabs retain that state in separate `ChatState` values. Updates carry the tab identity; applying a background result temporarily swaps in its state and then restores the visible chat. Background responses do not enter another chat.

Closing a tab stops only its local event subscription and sends no cancellation request to the daemon. Closing the last tab leaves a fresh blank tab. Closing the desktop does not automatically cancel daemon work either.

Editor and diff views reuse library components; file operations use the daemon workspace protocol. Switching chats caches and restores the open file and unsaved Editor text by session. A pending file save must finish before switching chats, creating a tab or closing the current tab.

After authentication, Desktop restores tab order, the active tab, each tab's composer draft and workspace path, panel visibility, dock layout, widths and appearance. Editing and navigation pause during restoration. Snapshots are coalesced and written in the background; normal window closure waits for the final flush. A tab whose original session is unavailable becomes an unbound draft. Use “为此草稿新建会话” (Create a session for this draft) to bind it before submission. Runs, events and approvals are fetched afresh from the daemon.

Unsaved Editor drafts retain the original session, path, base content and revision. Restoration checks the current file revision; a changed file leaves the draft intact and reports a conflict. Saving still validates the original revision in the daemon, protecting newer file content. Drafts with unavailable sessions remain unbound until explicitly attached. Restoration itself never saves a file.

Browser attachment, navigation, close and takeover requests must finish before creating, switching or closing the current chat, or reconnecting to the daemon. If a Space is created but opening its URL fails, the view retains that Space so the address can be corrected or the Space explicitly closed. A control change clears the old frame and requests a fresh one; late frames, empty results and errors from the previous frame generation cannot replace the new read state.

### Computer pane

Connect to the daemon and select an existing chat, then open Computer:

1. Choose Refresh targets to read the local catalog.
2. Explicitly choose a window or monitor in the library's searchable Select. Refresh leaves the selection empty rather than selecting the first item.
3. Choose Attach to selected chat to attach that exact descriptor to the selected chat.
4. Choose Detach when finished.

The pane separately displays Selected chat, Bound chat and Bound scope. Switching or closing chat tabs does not transfer the original connection; Bound chat continues to show its original session. Refresh and another target selection are unavailable while attached. Wait for target discovery, attachment or detachment to finish before switching, creating or closing chats, or reconnecting.

Starting a Connect attempt revokes the old host and clears the catalog even if authentication to the new connection fails. Refresh, chat binding and connection changes invalidate obsolete asynchronous results; a late attachment result is dropped and its host cancelled. Disconnects never replay actions. Explicitly Detach, refresh and choose again. Failed detachment warns that native calls may still be draining and another attachment can remain temporarily unavailable. Closing Desktop revokes the local host without automatically cancelling daemon jobs.

### Desktop shortcuts

`Cmd/Ctrl` below means either the `Cmd` or `Ctrl` key combination.

| Key | Action |
| --- | --- |
| `Cmd/Ctrl+B` | Toggle the session sidebar |
| `Cmd/Ctrl+Alt+B` | Toggle the right workbench |
| `Cmd/Ctrl+,` | Open Settings |
| `Esc` | Return from Settings to the chat |
| `Cmd/Ctrl+T` | Create a chat tab |
| `Cmd/Ctrl+W` | Close the current tab; return to the chat when in Settings |
| `Cmd/Ctrl+L` | Return to the chat and focus the composer |
| `Enter` / `Cmd/Ctrl+Enter` | Submit committed composer text; active IME composition does not submit |
| `Shift+Enter` | Insert a newline in the composer |
| `Cmd/Ctrl+S` | Save a file in Editor |

Native window startup depends on the target platform's graphical environment and dependencies. Compilation does not replace runtime checks for window creation, input, theme switching and connection to a real daemon, or establish acceptance on Windows, macOS and Linux.

## Local recovery files

Recovery files live under `client-state/` in the configured data directory. `agent-client::local_state` isolates them by client kind, canonical daemon endpoint and authenticated node identity. An exclusive lock prevents a second client instance from overwriting the same identity's file. Writes atomically replace the file using a temporary file in the same directory, with a 4 MiB size limit. On Unix, directory and file permissions are `0700` and `0600` respectively.

Snapshots contain client presentation and input state. They do not provide credentials, approval decisions, native execution handles, Browser/Computer control or daemon execution state. Restarting still requires authentication and fresh daemon queries; restoring a draft sends no request. Corrupt files, identity mismatches and unsupported versions are reported and preserved instead of being overwritten with empty state.

Entry points: `apps/agent-cli/src/main.rs`, `apps/agent-desktop/src/main.rs`. TUI presentation is defined in `apps/agent-cli/src/view_model.rs`, `view.rs` and `markdown.rs`; Desktop interaction is defined in `apps/agent-desktop/src/view_model.rs`, `operations.rs`, `projection.rs`, `chrome.rs`, `settings.rs`, `tabs.rs`, `tabs/` and `workbench/drafts.rs`.

See [Desktop screenshots and acceptance scope](runtime-evidence.md) for actual windows, light/dark switching, approval, file reading and their verification limits.
