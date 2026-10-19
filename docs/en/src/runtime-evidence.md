# Desktop screenshots and acceptance scope

These screenshots show the local native GPUI Desktop window on macOS 27.0.1, arm64, with GPUI Kit 0.7.3. The workspace locks CEF 154.0.34 (`cef` Rust crate 154.5.0+154.0.34). They record runs of the 0.1.0 native acceptance example, using a local HTTP model fixture through an authenticated connection to a real agentd. The fixture returns synthetic content without paid model calls. The eight images of Settings, conversation, tabs, collapsed panels, approval, Editor and Diff were captured on October 18, 2026. The [native capture record](evidence/desktop-native-capture.json) stores the executable, source and image digests, two Runs persisted as `completed`, and the exact file-write approval persisted as `consumed`. See [the client documentation](clients.md) for Settings, chat tabs and panel toggles.

The browser run used a debug-only Chromium test keychain; it does not establish acceptance of production keychain integration. These images do not establish Windows, Linux, public-network, real-model, native Computer input or final installer acceptance.

## Settings and native window

![Dedicated native Settings page on macOS](images/desktop-settings-macos.png)

The dedicated Settings page shows the connection address, masked access token, workspace and light/dark appearance options. The conversation titlebar does not show the address or token. Native interaction also observed the system macOS window controls and exercised the native zoom action; see the [native capture record](evidence/desktop-native-capture.json).

## Light and dark conversation

![Native GPUI conversation in light mode on macOS](images/desktop-chat-light-macos.png)

The light window shows a synthetic reply streamed through the daemon, the session list and the native composer. The right workbench is closed; this image provides no evidence of a running CEF page.

![Native GPUI conversation in dark mode on macOS](images/desktop-chat-dark-macos.png)

The same session after a manual switch to dark mode. This does not establish automatic synchronization with operating-system theme changes.

## Tabs and panel toggles

![Independent draft restored after returning to the first chat tab](images/desktop-tabs-macos.png)

Different drafts were entered in two tabs. Returning to the first tab restored `Draft retained in the first tab.` The [native capture record](evidence/desktop-native-capture.json) also records the accessibility observation of `A separate draft in the second tab.` restored in the other tab. Neither draft was submitted to the model.

![macOS conversation with both side panels collapsed](images/desktop-panels-collapsed-macos.png)

Native interaction collapsed and expanded each side independently. Both panels are collapsed in this image, while the titlebar retains the buttons that reopen them.

## Approval and file result

![Native GPUI window awaiting write_file approval on macOS](images/desktop-approval-macos.png)

The real daemon requests `write_file` for `notes.txt`. The window shows the exact path and content, with allow-once and deny buttons. This run completed an allow-once decision, with its approval persisted as `consumed` and its Run as `completed`; the screenshot records the state before approval.

![Native GPUI Editor reading the approved file on macOS](images/desktop-editor-macos.png)

After approval, Editor reads `notes.txt` through the daemon workspace interface and displays the actual written content. This image records file reading; separate component tests cover saving and conflict handling.

## Git diff

![Actual populated Git Diff in the native macOS window](images/desktop-diff-macos.png)

The acceptance workspace commits the original `README.md`, then modifies its tracked content. After connecting to the daemon, creating a session and refreshing Diff, the panel displays the actual Git patch in dark mode: it removes `Original tracked content.` and adds `A real change for the native Diff workbench.` The conversation on the left comes from the local model fixture in the same native-window run. This image records the unified diff view; no patch content was inserted manually.

The [capture record](evidence/desktop-diff-capture.json) includes executable and image digests, the actual patch and the Run identifier persisted as `completed`. The version 0.1.0 acceptance executable uses production native views and a local fixture; this image is separate from final installer acceptance.

## Embedded browser and human takeover

These two browser images share a separate native acceptance build, outside the build-digest scope of the eight images above. The [browser capture record](evidence/desktop-browser-capture.json) contains executable, source and image digests, the completed Run, and persisted Space open, takeover, release and close results.

![Real Chromium page displayed inside GPUI](images/desktop-browser-macos.png)

The CEF runtime loads a local static acceptance page and sends actual Chromium pixel frames through the daemon to the Browser panel. Agent control presents a read-only view.

![Real Chromium text and click result during human takeover](images/desktop-browser-takeover-macos.png)

Selecting Take control enters Human control. Native input entered `Native takeover verified`, followed by a page button click that displayed `Click reached real Chromium.`. Selecting Dark in Settings and returning to the chat retained both results, as shown above. Control was then returned to the agent and the Space closed. These images use the debug test keychain.

The address field retains navigation input; it is not a trusted display of the committed page origin after redirects. These images cover human browser interaction. Separate autonomous native-CEF acceptance uses an isolated synthetic page; see [Local tools and browser actions](tools.md).

A separate GPUI component test used the real daemon and CEF to verify pixel frames, pointer coordinates, text input, input rejection after releasing takeover, and Space closure. It used GPUI's test window and the debug acceptance executable's test keychain. This checks component calls and actual Chromium behavior separately from the native-window screenshots above.

The [separate check](evidence/browser-production-probe.json) of the packaged release CLI entry on October 19, 2026 failed: CEF started and returned frames and AX, but the target page stayed at `about:blank` during the wait. The cause remains undiagnosed; production navigation and keychain acceptance did not pass.

## Native Computer targets and session binding

These images were captured on October 19, 2026 using a separate GPUI acceptance executable with `native-computer` enabled. After the user granted screen recording and Accessibility permissions to this test app in macOS Settings, the Computer pane discovered targets and explicitly attached the acceptance window to an authenticated daemon session.

![Acceptance window attached to a fixed session in the macOS light appearance](images/desktop-computer-attached-macos.png)

![Original binding retained after changing chats in the macOS dark appearance](images/desktop-computer-dark-macos.png)

In the second image, Selected chat differs from Bound chat; the original window remains bound to the original session. An actual Detach then restored detached status and cleared the bound session and scope. The [Computer capture record](evidence/desktop-computer-capture.json) includes executable, relevant source and image digests. The isolated database contains two sessions and no Runs, approvals or operations. This verifies target discovery, attachment, chat isolation and detachment only. No Harness screen observation or native input was requested. Actual lock/unlock and user switching remain unverified.

## Actual terminal run records

![Actual TUI process awaiting exact file approval, recorded through a PTY](images/tui-pty-approval-macos.png)

![Completed TUI run after approving the file write](images/tui-pty-completed-macos.png)

These images come from an actual `agent-cli` process in an isolated 140-column, 38-row PTY on macOS. It authenticated to the local daemon, created a session, received streamed replies and approved the exact `notes.txt` write using `Ctrl+Y`. The second Run reached `completed`; exiting restored terminal state.

pyte 0.8.2 parsed the recorded ANSI output for terminal-state rendering. **These are not screenshots of an operating-system Terminal window.** Session, reply and approval content were not inserted manually. Synthetic replies reuse the Desktop fixture; references to GPUI, Files and Diff do not imply that those desktop panels exist in the TUI.

Inspect the [approval text](evidence/tui-pty-approval.txt), [completed text](evidence/tui-pty-completed.txt) and [capture metadata](evidence/tui-pty-capture.json). They establish this PTY workflow, without replacing native-terminal font, color or input-method acceptance.

## macOS package-content checks

The 0.1.0 ARM64 release build completed on macOS 27.0.1. Client builds enable `native-cef` and `native-computer`; agentd includes both `local-llama` and `local-mistral` CPU inference adapters. The [package verification record](evidence/native-packages-macos.json) lists build features, executable and packaging-tool digests, and the sizes and SHA-256 hashes of five artifacts.

| Format | Artifacts | Checks performed |
| --- | --- | --- |
| PKG | One each for CLI and Desktop | Component metadata, complete app trees, shared agentd, entry links and BOM inventories |
| tar.gz | One each for CLI and Desktop | Final archive entries match staging types, modes, contents and links |
| DMG | One for Desktop | Read-only mounted app tree matches staging; image detached afterward |

Each client's five Helpers match its main executable digest, and all 232 CEF files match the pinned SDK. Both PKGs are unsigned and the artifacts are not notarized. These records cover generation and package contents; operating-system installation, native service lifecycle, upgrades and rollback require separate runtime acceptance. See the [local distribution guide](https://github.com/lyonbrown4d/lyonbrown/blob/main/packaging/README.md) for build commands.

## Capabilities outside this screenshot set

These images do not show model-driven browser semantic actions or native terminal-window presentation. Synthetic-host protocol regressions and actual autonomous CEF acceptance have separate records. See [Local tools and browser actions](tools.md).
