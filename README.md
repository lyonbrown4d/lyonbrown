# Lyonbrown

Lyonbrown is a Rust workspace for a daemon-owned agent runtime, an authenticated terminal client and a native GPUI desktop. Rig's `AgentRun` owns the agent loop; model protocols, MCP tool integration and remote vector search reuse Rig's framework components. Clients present sessions and tasks. The daemon retains authorization, transactional persistence and uncertain-outcome recovery. The reusable decision engine and AI Gateway are embedded in agentd for model routing and access control.

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

The repository's [Cargo alias](.cargo/config.toml) invokes `apps/agent-dev`: `cargo dev tui` expands to `cargo run --locked -p agent-dev -- tui`. Cargo downloads and builds Rust dependencies from `Cargo.lock`; no `cargo install` step is needed for the launcher. Client commands build the required binaries, start or reuse the same development agentd and connect automatically. See the [development guide](docs/en/src/development.md) for configuration and shutdown options.

The [reusable decision library](docs/en/src/decision-engine.md) runs without a server or model weights. See the [Gateway guide](docs/en/src/gateway.md) for the embedded Gateway and its optional HTTP listener.

```sh
cargo run --locked -p decision-core --no-default-features --example embedded
cargo run --locked -p model-mock -- --help
```

Build the documentation with mdBook:

```sh
mdbook build docs/zh
mdbook build docs/en
```

The books use mdBook's native light and dark themes. Mermaid diagrams support zoom, pan and SVG download through bundled browser assets; no JavaScript build tool is required.

The guides describe source behavior and its limits. Native GUI, CEF, model and cross-device features require their own runtime acceptance; a workspace build alone does not establish that acceptance.
