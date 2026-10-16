# Release Artifacts

> Status: relocatable Windows x64 and macOS arm64 development artifacts are verified. They are not
> signed release candidate or installer.

The 2026-10-16 workspace implementation adds host-native packaging entry points
for Windows, macOS, and Linux x64/arm64. Each OS/architecture must build and pass
acceptance in its own environment; cross-compilation is not promised. Final
artifact acceptance records for the added platforms remain pending.

## Build contract

Run the packaging command from a clean repository root:

```text
pnpm package:release -- --output artifacts/lyonbrown-win32-x64
```

The output must be a new child of `artifacts/`; packaging refuses to overwrite an existing path.
The command builds `agent-cli` and its production dependency graph, uses pnpm's hoisted deploy
layout, embeds the currently pinned Node 24 runtime, and writes a relocatable `agent.cmd`
on Windows or executable `agent` shell launcher on macOS/Linux, using `runtime/node`. The TUI
resolves the daemon through the public `@lyonbrown/agentd/daemon` package entry instead of relying
on the monorepo directory layout.

The artifact contains no GGUF. Model inspection remains asynchronous, and a model is downloaded
only after an explicit user action.

Deployment generates a dedicated lockfile and enables workspace injection and file copying for
that command without changing global installation settings. Copying prevents later source builds
from changing an existing artifact through hard links. It avoids `--legacy`,
which can re-resolve dependencies and drift from the repository's locked Pi
dependency combination. Artifact smoke must verify independent daemon import.

On macOS/Linux, omit `--output` for the host-specific default or select a matching
directory such as `artifacts/lyonbrown-darwin-arm64`. Packaging uses moon's build
graph. After relocation, the launcher preserves arguments containing spaces,
Unicode, and shell metacharacters.

## Artifact smoke

Packaging runs this check automatically; it can also be repeated after copying or renaming the
artifact:

```text
pnpm smoke:release-artifact -- --artifact artifacts/lyonbrown-win32-x64
```

The smoke test uses the embedded runtime and verifies:

- the package manifest and CLI version agree;
- Windows artifacts contain no symlinks or junctions; POSIX allows only relative
  links to regular files inside the artifact, rejecting absolute, directory, or
  escaping links so resolution is not bound to the build path;
- the packaged daemon entry imports independently of the source workspace;
- packaged `better-sqlite3` opens an in-memory database;
- packaged `node-llama-cpp` loads a platform prebuilt binary with source builds and downloads disabled: macOS selects its native backend automatically; Windows/Linux verify the CPU backend.

On 2026-10-06, a Windows x64 development artifact passed the smoke, was renamed, and passed again.
That artifact contained 20,263 files and 943,666,133 bytes.

On 2026-10-16, a macOS arm64 development artifact passed locked dependency deployment,
embedded Node, independent daemon import, SQLite and native prebuilt smoke. It passed again
in a directory containing spaces and Chinese characters, including launcher version verification.
Deployed files were detached from source hard links. This acceptance did not download or load a
GGUF and does not replace real model task acceptance.

## Remaining release work

The current development artifact intentionally preserves the complete dependency closure. In
particular, `node-llama-cpp` contributes multiple optional CPU, Vulkan, and CUDA packages, so the
artifact is not yet size-optimized. The release design must decide whether to ship separate CPU,
Vulkan, and CUDA variants or one larger combined artifact, then verify that pruning cannot activate
a runtime build or download fallback.

Archive creation, checksums/SBOM, code signing, installer behavior, clean-machine validation,
upgrade/rollback/uninstall, Windows arm64 verification, and the fixed real-GGUF flow against the
final artifact remain required before release.
