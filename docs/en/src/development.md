# Development and verification

The workspace uses Rust edition 2024. Cargo workspace declarations and `Cargo.lock` manage product dependencies. The root `Cargo.toml` records the minimum Rust version in `rust-version`. Engineering constraints live in the root `AGENTS.md`.

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
