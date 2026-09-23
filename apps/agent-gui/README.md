# Lyonbrown Agent GUI

A native GPUI shell for Lyonbrown's local-first coding agent cockpit. This is a
UI template: it presents static sessions, a conversation, a task composer, and
runtime signals, but does not connect to `agent-kernel` or claim to execute work.

## Run

Use the latest stable Rust toolchain, then run from the workspace root:

```shell
cargo run -p agent-gui
```

Windows uses GPUI's Win32/DirectWrite backend. Linux requires the native
Wayland/X11 development libraries listed by GPUI; macOS requires Xcode command
line tools. The layout collapses the side rails below 900 px while keeping the
conversation and runtime-preview control available.

The runtime rail always reports `NOT CONNECTED`; its control only expands or
collapses explanatory preview details. It does not simulate model or tool work.

## Dependency note

The crate pins the official crates.io `gpui` 0.2.2 release. That release embeds
its platform construction and uses `Application::new()`. The separately split
`gpui_platform` crate documented on Zed's current `main` branch is not published
in the registry paired with this release, so mixing it into this crate would
produce incompatible or unresolvable dependencies.

GPUI is pre-1.0 and its API can change without a compatibility window. Treat
upgrades as explicit migrations: pin versions, read upstream changes, and run
the GUI checks on every supported OS before updating.
