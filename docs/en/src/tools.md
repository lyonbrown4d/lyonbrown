# Local tools and browser actions

agentd assembles model tools; clients display calls, approvals and results. Tool results, page content and process output cannot grant additional authority. A remote session's native commands execute on the daemon host.

## Enable trusted commands

`execute_command` is disabled by default. An operator explicitly lists absolute executable paths in the daemon's `config.toml`; restart the daemon to expose the tool. This macOS example allows `/usr/bin/printf` and passes only the selected environment reference to the child:

```toml
[commands]
executables = ["/usr/bin/printf"]

[commands.environment]
CHILD_KEY = "LYONBROWN_CHILD_VALUE"
```

`LYONBROWN_CHILD_VALUE` must exist in the dotenv or process environment read by layered configuration. The right-hand side names a variable rather than containing a credential. Unresolved references fail configuration loading. Children do not inherit the full environment. Arguments go directly to the executable without a shell. Executable and environment maps each allow up to 64 entries.

The working directory must remain inside the session workspace and defaults to `.`. This does not isolate filesystem or network access: a trusted program retains the operating-system rights of the user running agentd. Allowing an interpreter or shell permits its arguments to express broader actions; operators must assess the program and each exact argument list.

| Model argument | Default and limits |
| --- | --- |
| `executable` | Required; must match the absolute-path allowlist |
| `arguments` | Empty array; up to 256 strings |
| `cwd` | `.`, relative to the session workspace |
| `timeout_ms` | `30000`; `1..=120000` milliseconds |
| `output_limit_bytes` | `65536`; `1..=1048576` bytes |

Each execution requires human approval of the exact action, bound to the session, `owner_epoch`, run, call, action identity, resolved executable, arguments, directory, timeout and output limit. The daemon rechecks session ownership and run state, persists execution intent before starting the program, then saves the outcome receipt. Unconfirmed cancellation, execution outcomes or receipt persistence enter `Uncertain`. A disconnect or repeated call must not trigger automatic replay. Inspect the durable receipt and actual side effects before deciding what to do next.

Sources: `crates/agent-config/src/commands.rs`, `crates/agent-workspace/src/process/`, `apps/agentd/src/commands/`. Regression entry point: `apps/agentd/tests/command_tools.rs`.

## Model browser tools

`browser_observe` lists the current session's tabs or observes a selected tab, returning its snapshot, revision, accessibility data and optional screenshot artifact reference. Content is marked untrusted. Tab-list URLs omit queries and fragments; page text cannot act as system authorization.

`browser_act` takes a `call` object and supports:

| Operation | Required conditions |
| --- | --- |
| `open_space` | `profile_id` and an attached runtime host; optional `host_id` and `headless` |
| `open_tab` | A `space_id` in this session and an allowed URL |
| `close_space` | A `space_id` in this session |
| `click` / `fill` / `press` | `tab_id`, current `snapshot_id`, `revision` and an observed `element_ref` |
| `navigate` | `tab_id`, current `snapshot_id`, `revision` and the destination URL in `text` |

Mutations require exact-action human approval. The daemon checks the session, owner epoch, runtime host binding, cancellation and human takeover state. Human takeover suspends model mutations; model tools expose neither raw input nor takeover. Observe again after a stale snapshot. Inspect recorded outcomes before proceeding after uncertainty; automatic replay is forbidden. Profile persistence does not establish live-page recovery.

For model mutations, one local transaction consumes the exact approval, records browser intent, and writes events and Outbox. It rechecks that the run is executing and the session and owner epoch match. A failed transaction rolls back approval consumption and intent together. Only after commit is the command sent to the approved runtime host. Cancellation or disconnection while awaiting a receipt preserves `Uncertain` without replay. Human browser commands use a separate authenticated entry point and record the same kinds of intent and outcome receipts.

Native CEF semantic actions invalidate the old snapshot before scrolling, focusing, key input or navigation. After entering these steps, an action error or failed follow-up observation produces `Uncertain`, stops the current model Run and retains durable receipts for human reconciliation. Input, ownership and snapshot checks precede the action.

Human takeover input invalidates cached semantic observations before focus or dispatch. Errors after that boundary produce a durable `Uncertain` receipt without automatic replay. Successful input does not itself advance the frame revision, preserving paired down/up events; page changes may still invalidate that revision.

`apps/agentd/tests/browser_tools.rs` uses local HTTP model mocks and real protocol adapters for Responses, Chat Completions and Anthropic Messages. It covers approval, denial, ownership/session boundaries, cancellation and screenshot artifacts. Its runtime host is synthetic: it verifies the Harness-to-daemon tool contract and durable state, and does not replace autonomous browser acceptance with native CEF.

`apps/agentd/tests/native_browser_tools.rs` separately verifies the native chain. In a debug CEF environment on macOS 27.0.1 arm64, all three protocols completed real AX observation, exactly approved fill and click actions, local HTTP page effects and durable receipts. Three additional scenarios discarded the receipt after an actual successful click; cancellation and recovery preserved `Uncertain` without replay. The test uses synthetic pages, isolated profiles and the debug test keychain. It does not establish production keychain or other-platform acceptance.

`apps/agentd/tests/native_browser_post_effect.rs` verifies a post-click page error through the Responses protocol: one actual click produces one local HTTP effect, followed by an error from oversized page content. It checks durable `Uncertain` Run and browser receipts, stopped model continuation, recovery without replay and rejection of new Runs in the unresolved session. This case and the six scenarios above ran against a rebuilt and signed native host.

The same debug environment also passed `apps/agentd/tests/native_browser_human_input.rs`: paired human mouse input produced one page effect, persisted an `uncertain` receipt and matching Outbox, and recovered without replay or model execution. `crates/agent-browser-runtime/tests/cef_runtime.rs` verifies successful same-revision pointer input and committed text.

```sh
cargo test -p agentd --test browser_tools --test command_tools --locked
```

See [Desktop screenshots and acceptance scope](runtime-evidence.md) for native runtime evidence. Tool implementation is in `apps/agentd/src/browser/agent_tools/`; BrowserRuntime lifecycle contracts are described in [Protocol and platform capabilities](protocol.md).
## Delegating to paired nodes

`delegate_task` is disabled by default. Verify the complete TLS and Noise pairing fingerprint through the daemon's `peer-pair` operation, then list the permitted node, HTTPS origin and capabilities in `config.toml`. Discovery results and model output cannot create pairings or change target origins.

```toml
[[delegation_targets]]
node_id = "worker-example"
endpoint = "https://worker.example:7448"
capabilities = ["workspace.read"]
```

This declares a permitted route; it neither creates trust nor expands the durable pairing grant. Supported capabilities are `workspace.read` and `workspace.write.approval`, further restricted by the effective execution plan. Only plans permitting distributed execution expose this tool to the Harness. Worker child runs cannot delegate again.

The model selects `dispatch`, `get`, `reconcile`, `cancel` or `accept` inside `call`. A task must belong to the current parent Run, Session, owner and `owner_epoch`. Dispatch specifies the target, instructions, capabilities and timeout. The daemon derives stable task and attempt identities from the current model call so repeated calls cannot start another execution.

Dispatch, cancellation and result acceptance require separate exact-action approvals. Dispatch approval binds the origin, complete paired identity and task scope. Acceptance also binds the reconciled result's SHA-256 digest and content. Changes to pairing, Session ownership or the result invalidate an earlier approval. Worker content is always untrusted and cannot grant authority.

After durable intent exists, disconnection, cancellation or a lost receipt produces `Uncertain` and stops the Run for reconciliation of that identity. Reconciliation queries the existing task without dispatching it again. See [Clients](clients.md) for human management commands.

`apps/agentd/tests/delegation.rs` uses actual adapters for all three model protocols, local model mocks and both Master and paired mTLS Worker Harnesses. It covers dispatch, approvals and result acceptance, as well as denial, revocation, rekeying, cross-parent access, scope expansion and unknown outcomes without replay.
