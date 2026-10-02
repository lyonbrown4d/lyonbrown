# Lyonbrown Agent GUI

A native GPUI client for Lyonbrown's local-first coding agent cockpit. The GUI
uses the same authenticated gRPC daemon as the TUI; it never constructs an
embedded `agent-kernel`.

## Run

Use the latest stable Rust toolchain. Build the GUI and its sibling daemon
before running from the workspace root:

```shell
cargo build -p agent-gui -p agentd
./target/debug/agent-gui
```

On Windows, use the repository toolchain discovery script:

```powershell
.\scripts\build-windows.ps1 -CargoArgs @('build', '-p', 'agent-gui', '-p', 'agentd')
.\target\debug\agent-gui.exe
```

Running only `cargo run -p agent-gui` does not guarantee that the required
sibling `agentd` binary exists in a clean target directory.

Windows uses GPUI's Win32/DirectWrite backend. Linux requires the native
Wayland/X11 development libraries listed by GPUI; macOS requires Xcode command
line tools. The layout collapses the side rails below 900 px while keeping the
conversation and runtime-preview control available.

At startup the GUI loads the effective runtime configuration to calculate its
scope fingerprint, then discovers, reuses, or starts the sibling `agentd`
executable. `CompositionRoot::with_native_client` injects the authenticated
native client, and the cockpit watches its atomic remote-state feed after the
GPUI entity is created. There is no embedded fallback.

If secure discovery or daemon startup fails, the window still opens in an
explicit unavailable state and shows a bounded, single-line error. The composer
stays disabled until the client has connected and accepted a fresh snapshot for
the current daemon instance. On platforms where an owner-only discovery
boundary is not yet available, startup fails closed and the GUI shows that
failure instead of bypassing authentication.

The composer accepts native text input (including IME composition) and sends a
remote `StartTurn` only while the daemon connection has a fresh, compatible
snapshot. Submission runs off the GPUI render path. A rejected command leaves
the draft in place and displays a bounded single-line error so it can be retried.

## Dependency note

The crate pins the official crates.io `gpui` 0.2.2 release. That release embeds
its platform construction and uses `Application::new()`. The separately split
`gpui_platform` crate documented on Zed's current `main` branch is not published
in the registry paired with this release, so mixing it into this crate would
produce incompatible or unresolvable dependencies.

GPUI is pre-1.0 and its API can change without a compatibility window. Treat
upgrades as explicit migrations: pin versions, read upstream changes, and run
the GUI checks on every supported OS before updating.
