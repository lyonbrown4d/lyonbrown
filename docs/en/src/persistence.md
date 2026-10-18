# Sessions, tasks and recovery

Sessions preserve the Harness semantics needed to continue a conversation. Run records preserve orchestration state for individual user requests. SQLite stores these separately and writes events and outbox records alongside critical changes.

## Submission and execution

`accept_run` checks `owner_epoch` and deduplicates by `(session_id, idempotency_key)`. Repeating the same key and prompt returns the existing run; using a different prompt with that key fails. The `one_active_run` unique index prevents multiple unresolved runs within one session.

```mermaid
sequenceDiagram
    participant UI as Client
    participant D as agentd
    participant S as SQLite
    participant H as rpi-harness
    participant T as Host tool
    UI->>D: StartRun(session, prompt, key, epoch)
    D->>S: Persist accepted run + event + outbox
    S-->>D: Commit
    D-->>UI: Run ID
    D->>D: Decide and restrict execution plan
    D->>S: Persist execution_plan event
    D->>H: Execute prompt
    H->>S: Commit session and tool intent
    S-->>H: Commit
    H->>T: Authorized side effect
    T-->>H: Tool result
    H->>S: Commit session result + event + outbox
    D->>S: Persist terminal run state + event + outbox
    UI->>D: WatchRun / ListRuns
    D-->>UI: Durable events and result
```

The runtime delegates tools to host adapters, which own authorization, workspace confinement and cancellation. Harness provider retries are disabled; retry policy belongs to durable orchestration. Recording intent before a side effect allows recovery to distinguish unexecuted work from unknown outcomes.

## Disconnection and restart

After reconnecting, query runs in the original session. Events are ordered by run ID and `cursor`; `WatchRun.after_cursor` requests later events. A disconnected stream does not establish task failure and must not trigger prompt replay.

The store's `recover_interrupted` marks executing records without a durable terminal result as `uncertain`. Accepted work that has not started is handled separately from work already executing. Runtime recovery checks unfinished Harness tool records: a missing committed tool result produces `Uncertain` instead of guessing whether the side effect happened.

An `uncertain` run remains unresolved. Starting another run in the same session cannot bypass it. Preserve the database, events and external operation evidence, then reconcile the actual outcome before choosing the next action. The current protocol has no general-purpose endpoint for manually changing terminal state.

## Approvals

An approval records the tool, arguments, owning run and expiry. `decide_approval` accepts only unexpired requests whose run is still awaiting approval. Consumption rechecks the run, tool and arguments and changes the approval to `consumed`, preventing reuse of the same authorization.

## Master and worker

TaskStore checks `attempt_id` and `owner_epoch` when updating worker attempts or accepting results. A successful attempt first enters `awaiting_acceptance`; a task becomes `completed` only after explicit result acceptance.

```mermaid
stateDiagram-v2
    [*] --> accepted
    accepted --> awaiting_acceptance: Attempt succeeded
    accepted --> failed: Attempt failed
    awaiting_acceptance --> completed: Master accepts result
    awaiting_acceptance --> rejected: Master rejects result
    accepted --> cancel_requested: Cancellation requested
```

This diagram describes storage state boundaries. It does not establish cross-device scheduling, network-partition or public-relay acceptance. Source references: `crates/agent-storage/src/runs.rs`, `tasks.rs`, `approvals.rs` and `crates/agent-runtime/src/runtime.rs`.
