# Long-term memory

The daemon manages preferences, facts and procedures. The relational database owns content, state, revisions, sources, Run bindings and indexing jobs. Qdrant stores rebuildable vectors and retrieval metadata. A vector hit does not establish that a memory is current and grants no tool or node authority.

This page describes the current working tree. The SeaORM, Qdrant and memory-context changes have not completed compilation, tests or runtime acceptance in this iteration; see [Development and verification](development.md#current-dependency-resolution-limit).

## Scopes and states

| Scope | Server-derived identity |
| --- | --- |
| `user` | The daemon's persisted node identity |
| `project` | A database project record for the node and the canonical workspace path persisted by the Session |
| `session` | A persisted Session owned by the current node |

Clients select a scope kind, without choosing arbitrary scope IDs. Management requests without a Session can access only the current user scope. Project access retains the workspace identity stored by the Session; resolving a moved path or retargeted symlink does not transfer that authority.

Kinds are `preference`, `fact` and `procedure`. States are `proposed`, `active`, `superseded`, `rejected` and `deleted`. Retrieval accepts only unexpired active records. Explicit user operations may activate records; the library's model-proposal path creates only proposed records. Automatic extraction, model memory tools and a Desktop memory panel are not connected.

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

Embedding uses an OpenAI-compatible HTTP `/embeddings` endpoint and requires exactly one vector with the configured dimensions and finite values. The ID, model, dimensions and normalized API address determine its configuration revision; rotating credentials does not change that identity. `api_key_env` references dotenv or process-environment credentials, which do not enter memory records or status output. Remote endpoints require HTTPS; loopback HTTP is allowed. URLs cannot contain credentials, queries or fragments. `LYONBROWN_MEMORY_ENABLED` overrides the enabled flag.

### Select a vector backend

| Backend | Current integration and limits |
| --- | --- |
| `embedded` | Uses pinned `qdrant-edge` 0.9.0 `EdgeShard`, with no separate Qdrant process; defaults to `memory/<configuration-revision>` inside the daemon data directory |
| `qdrant` | Uses pinned `qdrant-client` 1.20.0 over gRPC; startup requires Qdrant Server `1.20.x` |

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

Missing collections are created with the named `memory` vector. Existing collections must match the configured dimensions and Cosine distance. The relational and vector backends are independent choices: SQLite, PostgreSQL or MySQL does not choose Qdrant mode. Changing embedding configuration or selecting a new index directory/namespace does not rerun completed jobs. Full-database reindex is not supported. Explicitly editing an active record creates a fresh revision and job; approving a proposed record activates it and creates a job. Index readiness does not establish successful inference by the embedding service.

## Management and indexing jobs

Authenticated `MemoryService` calls provide listing, details, save, forget, search, job listing, explicit retry and status. Clients use daemon identity and Session authorization, without directly accessing the database or Qdrant. See [Terminal and desktop clients](clients.md) for command entry points.

Each mutation saves the current record, revision, event, Outbox and indexing intent in one relational transaction. Active revisions enqueue upserts; other states enqueue deletes. One logical daemon must exclusively own each relational database; shared-database consumption and recovery by multiple daemons are unsupported. Its single consumer claims durable jobs, increments the attempt, and checks that attempt again when recording completion.

Job states are `pending`, `running`, `completed`, `failed` and `uncertain`. Startup moves interrupted running jobs to uncertain without replay. Failed and uncertain jobs require explicit retry. Successful metadata persistence does not imply a confirmed vector write. Index-write timeouts, cancellation and unconfirmed outcomes retain uncertainty. Without embedding configuration, the background consumer does not execute persisted index intents.

## Retrieval, revalidation and Run bindings

Search constrains both scope and embedding identity. Every hit is reread from the relational database to check its current revision, active state, expiry and authorization before deduplication and ranking. Stale, expired or deleted vectors cannot directly supply content.

Run preparation retrieves up to 16 records within a five-second budget and bounds injected UTF-8 content by `context_bytes`. Optional retrieval failure or timeout may be skipped; relational read failure prevents the model request. Before exposing content, a transaction rechecks the current record, Session ownership, owner epoch and active Run, then stores memory IDs, revisions, content hashes and `memory_bound` events. Revision changes discard that context attempt.

Selected text enters the system prompt as JSON explicitly marked as untrusted reference data. It cannot override the current user request, change tool policy or grant execution authority. A binding records what content identity was used; it does not certify factual truth.

Implementation sources: `crates/agent-config/src/memory.rs`, `crates/memory-core/`, `crates/agent-memory/`, `crates/agent-storage/src/memory/`, `crates/memory-qdrant/`, `apps/agentd/src/composition/memory.rs`, `apps/agentd/src/memory/`, `apps/agentd/src/memory_context.rs` and `apps/agentd/src/memory_worker.rs`.
