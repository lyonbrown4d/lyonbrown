# Lyonbrown

Lyonbrown is a Rust workspace for a daemon-owned agent runtime, an authenticated terminal client and a native GPUI desktop. `rpi-harness` owns the agent loop. Clients present sessions and tasks; they do not run a second loop.

- [中文文档](docs/zh/src/overview.md)
- [English guide](docs/en/src/overview.md)
- [Engineering constraints](AGENTS.md)

Build the documentation with mdBook:

```sh
mdbook build docs/zh
mdbook build docs/en
```

The books use mdBook's native light and dark themes. Mermaid diagrams support zoom, pan and SVG download through bundled browser assets; no JavaScript build tool is required.

The guides describe source behavior and its limits. Native GUI, CEF, model and cross-device features require their own runtime acceptance; a workspace build alone does not establish that acceptance.
