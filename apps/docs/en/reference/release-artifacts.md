# Release Artifacts

> Status: a relocatable Windows development artifact is implemented and verified. It is not a
> signed release candidate or installer.

## Build contract

Run the packaging command from a clean repository root:

```text
pnpm package:release -- --output artifacts/lyonbrown-win32-x64
```

The output must be a new child of `artifacts/`; packaging refuses to overwrite an existing path.
The command builds `agent-cli` and its production dependency graph, uses pnpm's hoisted deploy
layout, embeds the currently pinned Node 24 runtime, and writes a relocatable `agent.cmd`. The TUI
resolves the daemon through the public `@lyonbrown/agentd/daemon` package entry instead of relying
on the monorepo directory layout.

The artifact contains no GGUF. Model inspection remains asynchronous, and a model is downloaded
only after an explicit user action.

## Artifact smoke

Packaging runs this check automatically; it can also be repeated after copying or renaming the
artifact:

```text
pnpm smoke:release-artifact -- --artifact artifacts/lyonbrown-win32-x64
```

The smoke test uses the embedded runtime and verifies:

- the package manifest and CLI version agree;
- the complete artifact contains no symlink or Windows junction, so dependency resolution is not
  bound to the build path;
- the packaged daemon entry imports independently of the source workspace;
- packaged `better-sqlite3` opens an in-memory database;
- packaged `node-llama-cpp` loads a prebuilt CPU binary with source builds and downloads disabled.

On 2026-10-06, a Windows x64 development artifact passed the smoke, was renamed, and passed again.
That artifact contained 20,263 files and 943,666,133 bytes.

## Remaining release work

The current development artifact intentionally preserves the complete dependency closure. In
particular, `node-llama-cpp` contributes multiple optional CPU, Vulkan, and CUDA packages, so the
artifact is not yet size-optimized. The release design must decide whether to ship separate CPU,
Vulkan, and CUDA variants or one larger combined artifact, then verify that pruning cannot activate
a runtime build or download fallback.

Archive creation, checksums/SBOM, code signing, installer behavior, clean-machine validation,
upgrade/rollback/uninstall, Windows arm64 verification, and the fixed real-GGUF flow against the
final artifact remain required before release.
