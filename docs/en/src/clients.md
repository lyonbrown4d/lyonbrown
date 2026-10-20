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

The workbench contains sessions, conversation, tasks, approvals and a composer. Failed connections display `Disconnected` without simulated model replies. Failed submissions preserve the draft and are not replayed automatically. The matching draft is cleared only after the service confirms acceptance.

| Key | Action |
| --- | --- |
| `Ctrl+N` | Create a session using the current working directory |
| `Tab` | Select the next session and load its runs |
| `Enter` | Submit input to the selected session |
| `Shift+Enter` / `Alt+Enter` | Insert a newline in the composer |
| Arrow keys, `Home` / `End`, `Delete` | Move the cursor or edit text at the cursor |
| `Ctrl+R` | Reconnect to the daemon |
| `Ctrl+X` | Request cancellation of the active run |
| `Ctrl+Y` / `Ctrl+E` | Approve / deny the first pending approval |
| `PageUp` / `PageDown` | Scroll the conversation |
| `Ctrl+C` / `Ctrl+D` | Exit the client |

The composer uses `ratatui-textarea` 0.9.3 for cursor editing, multiline text, wrapping and paste. Application shortcuts take precedence over matching editor bindings: `Ctrl+X` still requests run cancellation and `Ctrl+Y` still handles approval. Pending submissions are not sent again; failure preserves the draft, and service acceptance clears the matching draft and its undo history.

The approval panel displays the tool and arguments. The daemon validates and saves decisions before the UI updates them. Durable runs refresh every two seconds; newly submitted runs also subscribe to an event stream. Exiting restores the terminal and sends no cancellation request.

`--theme auto` uses `COLORFGBG` to choose text colors while preserving the terminal background. Without a hint it uses a dark-terminal palette; select `--theme light` or `--theme dark` explicitly when needed.

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

## GPUI desktop

```sh
cargo run -p agent-desktop
```

The desktop uses GPUI and `gpui-kit` for the session sidebar, multiple chat tabs, conversation input and Browser, Computer, Files, Editor, Diff and Tasks dock panels. The custom title bar reuses the library's `TitleBar`; macOS uses native close, minimize and zoom controls. The session sidebar starts open and the right workbench starts closed. Title bar buttons toggle each side independently and remain available to restore either panel when both are hidden.

Background results use bounded `async-channel` 2.5.0 queues; producers await capacity when a queue is full. GPUI applies results on its main thread and discards replies for obsolete tabs or generations. Browser frames retain an independent 160 ms refresh timer, and Computer hosts retain an independent 80 ms health timer. Closing a window cleans up local subscriptions and hosts; daemon jobs retain their independent lifetime.

The ellipsis menu in the workbench tab bar lists all six panels: Browser, Files, Editor, Diff, Tasks and Computer. Use it to switch panels when a narrow window hides a tab. The component library still does not expose proper tab names in its accessibility tree; the menu does not resolve that tab accessibility limitation.

### Settings and connection

The title bar's Settings button opens a dedicated page. The daemon address and access token are edited only there; Workspace path selects the daemon-side folder for new chats in the current tab. The endpoint, token and workspace must match the daemon. A workspace for a remote daemon refers to that daemon's filesystem. Appearance offers manual Light and Dark choices. Back to chats and `Esc` return to the chat while preserving panel visibility and chat drafts.

Connect is unavailable while a run is active, a request is pending, or file edits are unsaved. Saving unchanged text also blocks reconnection until that save request finishes. The unsaved-edit check covers both the current Editor and cached file drafts for other sessions. While a connection request is pending, the old Editor remains read-only to prevent writes through the connection being replaced.

### Chat tabs and drafts

Use “＋ New chat” to create a chat tab. Selecting a session in the sidebar opens its tab or activates the existing tab for that session. Each tab has its own identity and retains its native composer, workspace path, input draft, messages, approvals and run state. Background tabs continue receiving their own run events; their responses do not appear in the visible chat.

Closing a tab stops only its local event subscription and sends no cancellation request to the daemon. Closing the last tab leaves a fresh blank tab. Closing the desktop does not automatically cancel daemon work either.

Editor and diff views reuse library components; file operations use the daemon workspace protocol. Switching chats caches and restores the open file and unsaved Editor text by session. A pending file save must finish before switching chats, creating a tab or closing the current tab. Tabs, drafts, panel visibility and appearance choices have no restoration guarantee across application restarts.

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
| `Cmd/Ctrl+Enter` | Submit input from the composer |
| `Cmd/Ctrl+S` | Save a file in Editor |

Native window startup depends on the target platform's graphical environment and dependencies. Compilation does not replace runtime checks for window creation, input, theme switching and connection to a real daemon, or establish acceptance on Windows, macOS and Linux.

Entry points: `apps/agent-cli/src/main.rs`, `apps/agent-desktop/src/main.rs`. Desktop interaction is defined in `apps/agent-desktop/src/lib.rs`, `chrome.rs`, `settings.rs`, `tabs.rs`, `tabs/` and `workbench/drafts.rs`.

See [Desktop screenshots and acceptance scope](runtime-evidence.md) for actual windows, light/dark switching, approval, file reading and their verification limits.
