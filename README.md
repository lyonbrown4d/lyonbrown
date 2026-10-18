# Lyonbrown

Lyonbrown is a Rust workspace for a daemon-owned agent runtime, an authenticated terminal client and a native GPUI desktop. `rpi-harness` owns the agent loop. Clients present sessions and tasks; they do not run a second loop.

- [中文文档](https://lyonbrown4d.github.io/lyonbrown/zh/)
- [English guide](https://lyonbrown4d.github.io/lyonbrown/en/)
- [Documentation sources](docs/)
- [Engineering constraints](AGENTS.md)

Run these development commands from the repository root; no additional task runner is required:

```sh
cargo dev desktop
cargo dev tui
cargo dev daemon
cargo dev --help
```

The repository's [Cargo alias](.cargo/config.toml) invokes `apps/agent-dev`. No extra task runner is required. Client commands build the required binaries, start or reuse the same development agentd and connect automatically. See the [development guide](docs/en/src/development.md) for configuration and shutdown options.

Build the documentation with mdBook:

```sh
mdbook build docs/zh
mdbook build docs/en
```

The books use mdBook's native light and dark themes. Mermaid diagrams support zoom, pan and SVG download through bundled browser assets; no JavaScript build tool is required.

The guides describe source behavior and its limits. Native GUI, CEF, model and cross-device features require their own runtime acceptance; a workspace build alone does not establish that acceptance.
