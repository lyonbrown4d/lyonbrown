# Long-term memory

The daemon manages preferences, facts and procedures. The relational database owns content, state, revisions, sources, Run bindings and indexing jobs. Qdrant stores rebuildable vectors and retrieval metadata. A vector hit does not establish that a memory is current and grants no tool or node authority.

This page describes the current working tree. The default-feature workspace compiles; tests, runtime and screenshot acceptance for memory tools, batched rebuilds and TUI/Desktop management have not been rerun; see [Development and verification](development.md#verification-scope).

## Scopes and states

| Scope | Server-derived identity |
| --- | --- |
| `user` | The daemon's persisted node identity |
| `project` | A database project record for the node and the canonical workspace path persisted by the Session |
| `session` | A persisted Session owned by the current node |

Clients select a scope kind, without choosing arbitrary scope IDs. Management requests without a Session can access only the current user scope. Project access retains the workspace identity stored by the Session; resolving a moved path or retargeted symlink does not transfer that authority.

Kinds are `preference`, `fact` and `procedure`. States are `proposed`, `active`, `superseded`, `rejected` and `deleted`. Retrieval accepts only unexpired active records. Explicit user operations may activate records. The model's `memory_propose` tool can create or update proposed records, but cannot activate or alter active records. There is no extraction outside explicit tool calls.

Writes check `expected_revision` and increment the revision by exactly one. Save, edit and activation requests deduplicate using a UUID `request_id`; reusing that identity with different content conflicts. Forget uses the memory ID and expected revision; job retry checks the job ID and state. Neither retries automatically: inspect the record or job state before acting on an unknown outcome. Deleted identities cannot be restored. Forgetting scrubs current and historical content and historical sources, records the deleted state and enqueues vector deletion. It does not retract context already sent to a model or securely erase old database pages, WAL, external backups or log copies.

## Configure retrieval

`memory.enabled` defaults to `true`. `context_bytes` defaults to `8192`, with an allowed range of `1024..=65536`. Embedding is unconfigured by default: relational management operations remain available, with no embedding calls, vector-index creation or synthetic vectors. Setting `enabled = false` also disables retrieval and the index consumer while retaining management operations.

Place this in the daemon's `config.toml`, using the [shared configuration](quickstart.md) precedence rules. The endpoint, model and dimensions must match a deployed service. This example has not been executed.

```toml
[memory]
enabled = true
context_bytes = 8192

[memory.embedding]
id = "local-embedding"
model = "deployed-embedding-model"
dimensions = 768
api_base = "http://127.0.0.1:8081/v1"
# api_key_env = "MEMORY_EMBEDDING_API_KEY"

[memory.vector]
backend = "embedded"
# directory = "memory/custom-index"
```

Embedding reuses Rig’s OpenAI-compatible embedding adapter and the shared bounded HTTP client for the `/embeddings` endpoint and requires exactly one vector with the configured dimensions and finite values. The ID, model, dimensions and normalized API address determine its configuration revision; rotating credentials does not change that identity. `api_key_env` references dotenv or process-environment credentials, which do not enter memory records or status output. Remote endpoints require HTTPS; loopback HTTP is allowed. URLs cannot contain credentials, queries or fragments. `LYONBROWN_MEMORY_ENABLED` overrides the enabled flag.

### Select a vector backend

| Backend | Current integration and limits |
| --- | --- |
| `embedded` | Uses pinned `qdrant-edge` 0.9.0 `EdgeShard`, with no separate Qdrant process; defaults to `memory/<configuration-revision>` inside the daemon data directory |
| `qdrant` | Uses pinned `qdrant-client` 1.20.0 over gRPC; startup requires Qdrant Server `1.20.x` |

Remote retrieval reuses Rig-Qdrant 0.46.0 `QdrantVectorStore::top_n` and `QdrantFilter`, supplying the existing query vector and scope filter without another embedding call. Results still require authoritative revision, state and authorization checks against relational storage.

Remote upserts and exact deletion by memory revision use single-write Qdrant SDK operations, retaining caller-selected stable identities and uncertain-outcome semantics. Index writes cannot generate replacement identities or retry autonomously through the framework. Rig exposes no Qdrant Edge interface, so the embedded backend uses the Edge SDK for the same Qdrant engine.

Relative embedded directories resolve inside the daemon data directory. A native ownership lock protects the index. Existing embedding descriptors, dimensions, Cosine distance and WAL settings must match. Synchronous embedded SDK work uses bounded blocking tasks; relational storage still uses its native asynchronous driver.

The embedding descriptor is published atomically before shard creation. An incomplete shard that cannot be loaded still returns an explicit error instead of being deleted or rebuilt automatically; relational memory content remains independent.

For a separate server, replace the vector configuration:

```toml
[memory.vector]
backend = "qdrant"
endpoint = "http://127.0.0.1:6334"
collection = "lyonbrown_memory"
# api_key_env = "MEMORY_QDRANT_API_KEY"
```

Missing collections are created with the named `memory` vector. Existing collections must match the configured dimensions and Cosine distance. The relational and vector backends are independent choices: SQLite, PostgreSQL or MySQL does not choose Qdrant mode. An embedding-descriptor change registers a batched rebuild for the currently authorized scopes. The default embedded directory includes the descriptor revision. Custom directories and remote collections still require compatible dimensions and descriptors; rebuilding does not bypass compatibility checks or delete a shard. Switching to an empty index with an unchanged embedding descriptor requires an explicit rebuild. A ready index does not establish real embedding inference acceptance.

## Management and indexing jobs

Authenticated `MemoryService` calls provide listing, details, save, forget, search, job listing, explicit retry and status. Clients use daemon identity and Session authorization, without directly accessing the database or Qdrant. See [Terminal and desktop clients](clients.md) for command entry points.

Each mutation saves the current record, revision, event, Outbox and indexing intent in one relational transaction. Active revisions enqueue upserts; other states enqueue deletes. One logical daemon must exclusively own each relational database; shared-database consumption and recovery by multiple daemons are unsupported. Its single consumer claims durable jobs, increments the attempt, and checks that attempt again when recording completion.

Job states are `pending`, `running`, `completed`, `failed` and `uncertain`. Startup moves interrupted running jobs to uncertain without replay. Failed and uncertain jobs require explicit retry. Successful metadata persistence does not imply a confirmed vector write. Index-write timeouts, cancellation and unconfirmed outcomes retain uncertainty. Without embedding configuration, the background consumer does not execute persisted index intents.

### Batched rebuilds

A rebuild persists its request before scanning authoritative records in memory-ID order, at most 100 records per batch. Each batch commits its cursor, counts, new index jobs, events and Outbox together in the relational database. It does not load all memories into RAM or write Qdrant while registering a request. An interrupted scan resumes from its committed cursor; generated index jobs retain their existing attempt and uncertain-outcome rules.

Requests retain their authorized scopes and full embedding descriptor. Worker startup checks the user scope; Session access checks the current user, project and session scopes. First use or a descriptor change registers a request. Reuse requires the latest request for those scopes to have the same descriptor, so switching from A to B and back to A scans again. Rebuild child jobs bind their target descriptor and cannot be claimed by a worker using another embedding configuration. Historical sessions and projects register on access rather than bypassing scope checks during startup. An explicit rebuild can scan the current scopes again with an unchanged descriptor, for example after index loss.

A rebuild stays `pending` until scanning and every generated index job finish. It becomes `completed` only when all child jobs are completed. Failed or uncertain child jobs retain their state and require inspection and explicit retry for that memory. Retry is explicitly rejected when a bound job's embedding descriptor differs from the current configuration; restore the original configuration to process it rather than executing it with a new embedding model. A new rebuild neither replays unknown jobs from an older request nor waits for them before scanning its own records. Rebuild progress describes index execution, not acceptance of embedding quality or performance.

### TUI and Desktop entry points

The TUI shows `/memory` results in its local record view. Listing, search, creation, detail, editing, activation, forgetting and job commands are documented under [client commands](clients.md). `/memory rebuild` submits one explicit request for the current authorized scopes. `/memory rebuilds [AFTER_ID]` lists progress in pages of 20. Inspect progress after timeout or disconnect; requests are not resubmitted automatically.

Desktop management is under `Settings → Memory`. With no Session, it shows user memories; a locally owned Session also enables its project and session scopes. The page supports pagination, active-memory search, creation of preferences/facts/procedures, content edits, explicit proposal activation or rejection, permanent forgetting, index-job inspection and retry, and a rebuild of the current scopes. Edits preserve an existing record's scope and kind and use its displayed revision for concurrency checks. Connection or Session changes clear old results and inputs; old asynchronous results cannot replace a newer scope. Native UI execution and screenshots remain unverified.

### Model memory tools

Tools enter only the set already permitted by the host. Classifier or model output cannot widen that set or select arbitrary scope IDs.

| Tool | Current behavior |
| --- | --- |
| `memory_read` | Reads one current, unexpired active record within the authorized scopes |
| `memory_search` | Searches by authorized scopes and embedding descriptor, then revalidates authoritative revisions and states |
| `memory_propose` | Accepts a scope kind, memory kind, content and optional ID/expected revision; the host binds Session, Run, source event and owner epoch, and writes only `proposed` records |

Proposal writes recheck Session ownership, Run and source-event authority in the transaction. Memory text and proposals cannot grant tool, model or node authority or replace human approval. Unavailable reads and searches return an error rather than placeholder memories or synthetic vectors.

## Retrieval, revalidation and Run bindings

Search constrains both scope and embedding identity. Every hit is reread from the relational database to check its current revision, active state, expiry and authorization before deduplication and ranking. Stale, expired or deleted vectors cannot directly supply content.

Run preparation retrieves up to 16 records within a five-second budget and bounds injected UTF-8 content by `context_bytes`. Optional retrieval failure or timeout may be skipped; relational read failure prevents the model request. Before exposing content, a transaction rechecks the current record, Session ownership, owner epoch and active Run, then stores memory IDs, revisions, content hashes and `memory_bound` events. Revision changes discard that context attempt.

Selected text enters the system prompt as JSON explicitly marked as untrusted reference data. It cannot override the current user request, change tool policy or grant execution authority. A binding records what content identity was used; it does not certify factual truth.

Implementation sources: `crates/agent-config/src/memory.rs`, `crates/memory-core/`, `crates/agent-memory/`, `crates/agent-storage/src/memory/`, `crates/memory-qdrant/`, `apps/agentd/src/composition/memory.rs`, `apps/agentd/src/memory/`, `apps/agentd/src/memory_context.rs`, `apps/agentd/src/memory_worker.rs`, `apps/agent-cli/src/memory_command.rs` and `apps/agent-desktop/src/resources/memory/`.
