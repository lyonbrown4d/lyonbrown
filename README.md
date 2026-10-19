# Lyonbrown

Lyonbrown is a Rust workspace for a daemon-owned agent runtime, an authenticated terminal client and a native GPUI desktop. `rpi-harness` owns the agent loop. Clients present sessions and tasks; they do not run a second loop. The reusable decision engine is embedded by default; an independent AI Gateway provides authenticated model routing and a component-based Leptos administration UI with server rendering and Rust/WASM hydration.

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

The [reusable decision library](docs/en/src/decision-engine.md) runs without a server or model weights. The [AI Gateway](docs/en/src/gateway.md) has its own Cargo launcher, SQLite data and Swagger UI:

```sh
cargo run --locked -p decision-core --no-default-features --example embedded
cargo gateway --help
cargo gateway --config /absolute/path/gateway.toml --data-dir /absolute/path/gateway-data
cargo run --locked -p model-mock -- --help
```

Gateway startup requires a configured super-administrator username/password and an independent API key; there are no default credentials. The launcher prepares cargo-leptos 0.3.11 under `.local/tools` and the current Rust toolchain's `wasm32-unknown-unknown` target, then builds the server and browser assets. No manual global tool installation or separate Node stack is required. Generated JavaScript boots the Rust/WASM UI. See the [Gateway guide](docs/en/src/gateway.md) for `.env`, `--dry-run`, `--build-only`, release builds and binary-plus-assets distribution.

Build the documentation with mdBook:

```sh
mdbook build docs/zh
mdbook build docs/en
```

The books use mdBook's native light and dark themes. Mermaid diagrams support zoom, pan and SVG download through bundled browser assets; no JavaScript build tool is required.

The guides describe source behavior and its limits. Native GUI, CEF, model and cross-device features require their own runtime acceptance; a workspace build alone does not establish that acceptance.
