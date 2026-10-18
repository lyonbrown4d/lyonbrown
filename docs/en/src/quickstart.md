# Run locally

Install the Rust toolchain required by the root `Cargo.toml`. Models, native graphics and browser SDK prerequisites depend on enabled capabilities; a crate name does not establish availability without the relevant native dependencies.

## Start the development environment

Choose a client with Cargo from the repository root. No additional task runner is required:

```sh
cargo dev tui
# Or open the desktop client:
cargo dev desktop
```

The command builds the required binaries, starts or reuses a development agentd and supplies the client connection settings. Multiple terminals and Desktop can share the default `.local/dev/agentd` data directory and `127.0.0.1:7547` endpoint. See [Development and verification](development.md) for data, configuration and shutdown options.

## Run the daemon independently

Run from the repository root:

```sh
cargo run -p agentd -- --data-dir /path/to/lyonbrown-data
```

The default listener is `127.0.0.1:7447`; the local client endpoint accepts loopback addresses only. The daemon stores node identity, `auth.token` and `agent.sqlite3` in the data directory and holds an exclusive lease on it. A second daemon using the same directory fails, preventing concurrent node-state writers.

Model requests originate in the daemon. Set `OPENAI_API_KEY` through the process environment or a `.env` adjacent to the configuration file before startup; use `ANTHROPIC_API_KEY` with the `anthropic` provider. Do not commit real credentials. Real-model requests cannot execute without provider credentials.

| Daemon option | Environment variable | Default or range |
| --- | --- | --- |
| `--listen` | `LYONBROWN_LISTEN` | `127.0.0.1:7447`; loopback only |
| `--data-dir` | `LYONBROWN_DATA_DIR` | System user-local data directory |
| `--provider` | `LYONBROWN_PROVIDER` | `openai`; also supports `openai-responses`, `openai-completions`, `anthropic` |
| `--model` | `LYONBROWN_MODEL` | `gpt-4.1`; availability depends on provider and credentials |
| `--api-base` | `LYONBROWN_API_BASE` | Provider-specific address when unset |
| `--max-runs` | `LYONBROWN_MAX_RUNS` | `4`; accepts `1..=128` |
| `--run-timeout-secs` | `LYONBROWN_RUN_TIMEOUT_SECS` | `900` seconds; accepts `1..=86400` |

The shared `agent-config` uses `config` and `dotenvy`. Configuration merges defaults, TOML, the `.env` adjacent to that file, process environment and explicit CLI arguments in that order. The default file is `config.toml` in the data directory; `--config-file` or `LYONBROWN_CONFIG` selects another file. Dotenv parsing does not mutate the global process environment. Provider endpoints require HTTPS, with HTTP allowed for loopback addresses. URLs cannot contain credentials, query parameters or fragments. The API mode must match the endpoint.

`supports_images` defaults to `false`. Enable `--supports-images` or `LYONBROWN_SUPPORTS_IMAGES=true` only when both provider and model support image inputs. See [Models and execution decisions](models.md) for classification settings, a TOML example and protocol mappings.

All product processes use shared `agent-telemetry`, built on `tracing`, `tracing-subscriber` and `tracing-appender`, for JSON-line logs in the data directory’s `logs` folder by default. Files rotate daily, retaining 14 files per process prefix. `LYONBROWN_LOG_DIRECTORY` and `LYONBROWN_LOG_RETENTION` override these settings; retention accepts `1..=365`. Credentials stay out of UI models and configuration debug output.

## Run clients directly and use native services

For development, `cargo dev tui` or `cargo dev desktop` automatically starts and connects to the shared development daemon. See [Development and verification](development.md) for options. Installed terminal and Desktop clients use the shared launcher at the default local endpoint to reuse the same user's native service after checking version, protocol and capabilities. macOS uses launchd, Linux uses a systemd user service, and Windows uses an SCM identity derived from the current user's SID.

The service registration lock and `service.json` always live in the default user data directory, regardless of a client's `--data-dir`. If an existing service uses a different data directory, the launcher refuses replacement; changing the client directory cannot register a second service with the same identity. The authentication token remains in the daemon's actual data directory.

On Windows, install the MSI, then select **Set up Lyonbrown service** in the Start menu. Setup requires same-account UAC elevation and collects the service login password in the native credential dialog; switching to another administrator account is rejected. CLI and Desktop share a protected Program Files daemon. Passwords never enter command-line arguments or configuration files. Upgrades wait for active tasks and check storage compatibility and rollback conditions.

Using `cargo run` to connect to the default local endpoint also checks the native service, so it requires the complete installed daemon or a built `agentd` beside the client binary. Prefer the `cargo dev` commands above during development. To run the terminal directly:

```sh
cargo run -p agent-cli -- --endpoint http://127.0.0.1:7447 --data-dir /path/to/lyonbrown-data
```

Clients receive tokens through `--token`, `LYONBROWN_TOKEN` or the adjacent dotenv file. At the default local endpoint, the shared launcher reads `auth.token` from the reused daemon’s data directory; other endpoints require an explicit token. Keep tokens out of documentation, Git and chat history. Environment variables or local files avoid placing the value in command history.

| Option | Environment variable | Default or behavior |
| --- | --- | --- |
| `--endpoint` | `LYONBROWN_ENDPOINT` | `http://127.0.0.1:7447` |
| `--token` | `LYONBROWN_TOKEN` | Local default uses the launcher’s `auth.token`; other endpoints require a token |
| `--data-dir` | `LYONBROWN_DATA_DIR` | Local data directory from `ProjectDirs::from("org", "Lyonbrown", "Lyonbrown")` |
| `--theme` | None | `auto`; also accepts `light`, `dark` |

Use the same data directory as the daemon, or provide the same token through the environment. On authentication failure, check the endpoint and token before submitting another prompt.

Once connected, press `Ctrl+N` to create a session, enter a message and press `Enter`. Results and approvals come from the daemon. After closing the client, reconnect to the original session to query results. See [Terminal and desktop clients](clients.md) for all controls.

Configuration sources: `apps/agentd/src/config.rs`, `crates/agent-config/src/load.rs`, `crates/agent-runtime/src/provider.rs`, `apps/agent-cli/src/config.rs`.
