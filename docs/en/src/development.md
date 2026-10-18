# Development and verification

The workspace uses Rust edition 2024. Cargo workspace declarations and `Cargo.lock` manage product dependencies. The root `Cargo.toml` records the minimum Rust version in `rust-version`. Engineering constraints live in the root `AGENTS.md`.

Async entry points prefer `#[tokio::main]`. Clap, Serde and fluxdi macros provide argument parsing, serialization and dependency injection. CEF helper and Windows SCM dispatch run before runtime initialization. GPUI retains its main-thread event loop; explicitly owned or shared runtimes remain part of its composition.

## Run locally during development

The repository's `.cargo/config.toml` defines the `dev` alias. It uses Cargo directly, with no additional task runner to install. Run one command from the repository root to build the required binaries, start a development agentd and open the client:

```sh
cargo dev desktop
# Or open the terminal UI:
cargo dev tui
```

Run `cargo dev tui` in multiple terminals alongside Desktop. Clients using the same data directory, address and configuration share one development agentd. The command supplies the connection address and credentials automatically; no separate daemon startup is required.

Defaults are `.local/dev/agentd` for persistent development data and `http://127.0.0.1:7547` for the endpoint. `cargo clean` preserves the data. These commands supervise foreground development processes without registering or replacing OS services. The terminal that first starts the daemon keeps its supervisor running after its client closes. Press Ctrl+C there to stop the development daemon. Later terminals reuse the daemon and can exit their clients without stopping it.

Startup checks are serialized for each data directory to prevent concurrent clients from creating multiple daemons. Reuse requires an authenticated handshake and matching node identity, protocol, build and configuration. An endpoint occupied by an unrelated process, invalid credentials or a different live build/configuration produces an error and leaves the existing process running. To use a new build or configuration, stop the development daemon in its original terminal and run the command again.

Relative paths resolve from the repository root and may contain spaces. Use matching options for all clients. Development requires a nonzero loopback port other than the product default 7447:

```sh
cargo dev desktop --dry-run
cargo dev tui --data-dir .local/dev/demo --listen 127.0.0.1:7548 --config /absolute/path/dev.toml
cargo dev desktop --data-dir .local/dev/demo --listen 127.0.0.1:7548 --config /absolute/path/dev.toml
cargo dev --help
```

The Cargo alias invokes `apps/agent-dev` with an argument array, without a platform shell or an extra task runner. The root `README.md` lists common commands; `cargo --list` also displays the `dev` alias.

Use `cargo dev daemon` when a separate foreground daemon is useful. Other development clients automatically connect to and reuse it.

The default configuration is `config.toml` inside the development data directory, with `.env` beside it. `--config` selects an existing TOML file. The commands do not create or overwrite configuration or dotenv files. Model, tool and log settings use the shared `agent-config` layering rules. agentd, TUI, Desktop and the development launcher all use shared `agent-telemetry` logging. Files default to the development data directory's `logs` subdirectory, use separate process prefixes and follow the configured rotation and retention settings. The launcher forces the selected development address and data directory, reads that directory's `auth.token` and ignores inherited connection credentials. Tokens pass only through the client child environment, never command arguments or launch output.

`--dry-run` displays the launch plan without creating directories, reading tokens or starting product processes; the outer Cargo command may compile `agent-dev`. Default Desktop/TUI builds do not enable `native-cef`; browser execution reports unavailable. See the [local distribution guide](https://github.com/lyonbrown4d/lyonbrown/blob/main/packaging/README.md) for the full CEF SDK, helper and application bundle layout.

## Verification commands

Run from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
mdbook build docs/zh
mdbook build docs/en
```

These commands check formatting, static issues, tests, compilation and documentation generation. They do not replace real-model requests, GUI startup, CEF operation, system permissions or cross-device acceptance. Report the command, environment, scenario and remaining coverage limits together.

## Module changes

Clients own presentation and input. Models, tools, permissions and durable state remain in the daemon. Add shared fields to `proto/agent/v1/agent.proto` and update both consumers; do not put execution-library internals in protocol messages.

Assess responsibility-based decomposition when a handwritten source file reaches 300 lines. Integration tests belong in each crate's `tests/` directory. Avoid production `panic`, `unwrap` and uncontrolled `unsafe`. Necessary FFI stays at platform adapter boundaries with its safety conditions documented.

## Maintain the books

```sh
mdbook serve docs/zh --open
mdbook serve docs/en --open
```

Both languages use matching chapter structures with separate prose. Describe source mechanisms, configuration and limits; distinguish source implementation, compilation, tests and runtime acceptance. Keep mdBook's native theme. Shared diagram extensions are in `docs/theme`; browser dependency versions and licenses are recorded in `docs/theme/VENDOR.md`.

Use Mermaid code fences for new diagrams. The browser adapter provides rendering and image interaction without an additional documentation preprocessor. After changing shared diagram assets, build both books and check light/dark themes, zoom, pan, keyboard dismissal and SVG download.

## Model protocol and decision regression

`agent-test-support` provides a shared local HTTP mock server. Integration tests in `agent-runtime` and `agent-decision` use the real protocol adapters for Responses, Chat Completions and Anthropic Messages without paid model calls. They cover structured output, authority restrictions, low confidence, conservative fallback, timeout and cancellation. Daemon tests also inspect actual tool behavior and durable events; mock output alone does not establish successful execution.

```sh
cargo test -p agent-runtime -p agent-decision -p agentd --locked
cargo test -p agent-mcp --features test-fixtures --locked
```

`apps/agentd/tests/computer_tools.rs` uses local model mocks, the real adapters for all three standard protocols, the Harness and an injectable synthetic `attach_host`. It checks observation, exact approval and denial, renewal through model and approval latency, configuration/host/input-permission restrictions, and durable `Uncertain` with no replay after lost or corrupted receipts and recovery. Assertions inspect input counts, approvals, execution receipts, events and Outbox. These tests perform no OS input and do not establish CLI/Desktop native attachment or desktop-control acceptance on three platforms.

```sh
cargo test -p agentd --test computer_tools --locked
```

Real local-model tests are explicitly ignored and require an existing chat GGUF file. Each command checks only the selected backend, model and current host:

```sh
LYONBROWN_TEST_GGUF=/absolute/path/model.gguf cargo test -p agent-inference --features llama --test native_models -- --ignored --nocapture
LYONBROWN_TEST_GGUF=/absolute/path/model.gguf cargo test -p agent-inference --features mistral --test native_models -- --ignored --nocapture
```

## Native service acceptance

The following macOS test registers a real launchd service. Use a test account with no Lyonbrown service registration and an unused `127.0.0.1:7447`. Build the product daemon, then provide its absolute path:

```sh
cargo build -p agentd --bin agentd --locked
LYONBROWN_SERVICE_AGENTD=/absolute/path/agentd cargo test -p agent-launch --test native_service --locked -- --ignored --nocapture --test-threads=1
```

The test checks that two clients reuse the same service, another data directory cannot replace it, and a native service restart preserves node identity and session records. On success it verifies registration ownership and idle state before removing its own service and data. On failure it preserves the data so a live daemon keeps its executable and store; inspect `service.json` and native service state before handling any remaining registration.

This path passed with the actual product binary on macOS 27.0.1, arm64, without model calls or Keychain authorization. It does not replace OS installation acceptance for the PKG or establish Windows SCM or Linux systemd runtime behavior.

## Publish the documentation site

Documentation is built locally and published from a GitHub Pages branch. Run `mdbook build docs/zh` and `mdbook build docs/en`. Place `docs/index.html` at the publication root, put the two generated books under `zh/` and `en/`, and add an empty `.nojekyll` file at the root. Cargo artifacts, local configuration and credentials are excluded.

Commit the static files to the separate `gh-pages` branch. Configure Pages with build type `legacy`, using `/` on that branch. Check both languages' local resources and links before pushing, then inspect the Pages build and read back the public site. A successful branch push alone does not establish a successful deployment.

`.github/workflows/pages.yml` remains a manual publishing entry for environments with GitHub Actions enabled; switch the Pages source to GitHub Actions before using it. The manual branch deployment does not invoke that workflow.
