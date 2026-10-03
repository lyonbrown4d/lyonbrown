import { isAbsolute } from "node:path";

import Database from "better-sqlite3";
import { Kysely, SqliteDialect } from "kysely";

import { appendEvents } from "./append.js";
import {
  CorruptEventStoreError,
  EventStoreClosedError,
  InvalidStoreOptionsError,
} from "./errors.js";
import {
  validateEventRows,
  validateSessionIntegrity,
  type RuntimeEventRow,
  type SessionIntegrityRow,
} from "./integrity.js";
import { defaultPathVerifier, prepareDatabasePath, reverifyDatabasePath } from "./path-security.js";
import { migrate, SCHEMA_VERSION, type EventStoreDatabase } from "./schema.js";
import type {
  EventStore,
  EventToAppend,
  ListSessionsOptions,
  LoadOptions,
  OpenEventStoreOptions,
  SessionMetadata,
  StoredEvent,
  StoreDiagnostics,
} from "./types.js";
import { validateList, validateLoad } from "./validation.js";

const SESSION_INTEGRITY_SELECT = `
  SELECT s.session_id, s.current_revision, s.created_at, s.updated_at,
         COALESCE(MAX(e.revision), 0) AS max_revision,
         COUNT(e.revision) AS event_count
    FROM sessions s
    LEFT JOIN events e ON e.session_id = s.session_id
`;

class SqliteEventStore implements EventStore {
  readonly #database: Database.Database;
  readonly #kysely: Kysely<EventStoreDatabase>;
  readonly #busyTimeoutMs: number;
  #closed = false;

  public constructor(database: Database.Database, busyTimeoutMs: number) {
    this.#database = database;
    this.#busyTimeoutMs = busyTimeoutMs;
    this.#kysely = new Kysely<EventStoreDatabase>({ dialect: new SqliteDialect({ database }) });
  }

  // better-sqlite3 commits synchronously; async keeps the storage-port contract stable.
  // eslint-disable-next-line @typescript-eslint/require-await
  public async append(
    sessionId: string,
    expectedRevision: bigint,
    events: readonly EventToAppend[],
  ): Promise<readonly StoredEvent[]> {
    this.#assertOpen();
    return appendEvents(this.#database, this.#busyTimeoutMs, sessionId, expectedRevision, events);
  }

  public async load(sessionId: string, options: LoadOptions): Promise<readonly StoredEvent[]> {
    this.#assertOpen();
    validateLoad(sessionId, options);
    const metadata = this.#sessionIntegrity(sessionId);
    if (metadata === undefined) return [];
    const rows = await this.#kysely
      .selectFrom("events")
      .selectAll()
      .where("session_id", "=", sessionId)
      .where("revision", ">", options.afterRevision)
      .orderBy("revision", "asc")
      .limit(options.limit)
      .execute();
    if (options.afterRevision < metadata.currentRevision && rows.length === 0) {
      throw new CorruptEventStoreError("Event stream ended before its recorded current revision");
    }
    return validateEventRows(rows as readonly RuntimeEventRow[], sessionId, options.afterRevision);
  }

  // better-sqlite3 returns the validated aggregate synchronously.
  // eslint-disable-next-line @typescript-eslint/require-await
  public async currentRevision(sessionId: string): Promise<bigint> {
    this.#assertOpen();
    validateLoad(sessionId, { afterRevision: 0n, limit: 1 });
    return this.#sessionIntegrity(sessionId)?.currentRevision ?? 0n;
  }

  // better-sqlite3 returns a committed point-in-time aggregate synchronously.
  // eslint-disable-next-line @typescript-eslint/require-await
  public async listSessions(options: ListSessionsOptions): Promise<readonly SessionMetadata[]> {
    this.#assertOpen();
    validateList(options);
    const rows = this.#database
      .prepare<[number, number], SessionIntegrityRow>(
        `${SESSION_INTEGRITY_SELECT}
        GROUP BY s.session_id, s.current_revision, s.created_at, s.updated_at
        ORDER BY s.updated_at DESC, s.session_id ASC
        LIMIT ? OFFSET ?`,
      )
      .all(options.limit, options.offset ?? 0);
    return rows.map(validateSessionIntegrity);
  }

  public diagnostics(): StoreDiagnostics {
    this.#assertOpen();
    return {
      journalMode: String(this.#database.pragma("journal_mode", { simple: true })),
      schemaVersion: Number(this.#database.pragma("user_version", { simple: true })),
      synchronous: Number(this.#database.pragma("synchronous", { simple: true })),
      busyTimeoutMs: Number(this.#database.pragma("busy_timeout", { simple: true })),
    };
  }

  public close(): void {
    if (this.#closed) return;
    this.#closed = true;
    this.#database.close();
  }

  #sessionIntegrity(sessionId: string): SessionMetadata | undefined {
    const row = this.#database
      .prepare<[string], SessionIntegrityRow>(
        `${SESSION_INTEGRITY_SELECT}
        WHERE s.session_id = ?
        GROUP BY s.session_id, s.current_revision, s.created_at, s.updated_at`,
      )
      .get(sessionId);
    return row === undefined ? undefined : validateSessionIntegrity(row);
  }

  #assertOpen(): void {
    if (this.#closed) throw new EventStoreClosedError();
  }
}

function configure(database: Database.Database, memory: boolean, busyTimeoutMs: number): void {
  database.defaultSafeIntegers(true);
  database.pragma(`busy_timeout = ${String(busyTimeoutMs)}`);
  database.pragma("foreign_keys = ON");
  database.pragma("synchronous = FULL");
  if (!memory) database.pragma("journal_mode = WAL");
  migrate(database);
  if (Number(database.pragma("user_version", { simple: true })) !== SCHEMA_VERSION) {
    throw new CorruptEventStoreError("Schema migration did not reach the expected version");
  }
}

export function openEventStore(options: OpenEventStoreOptions): EventStore {
  const busyTimeoutMs = options.busyTimeoutMs ?? 5000;
  if (!Number.isSafeInteger(busyTimeoutMs) || busyTimeoutMs < 0 || busyTimeoutMs > 60_000) {
    throw new RangeError("busyTimeoutMs must be a safe integer from 0 to 60000");
  }

  if (options.testOnlyMemory === true) {
    const database = new Database(":memory:");
    configure(database, true, busyTimeoutMs);
    return new SqliteEventStore(database, busyTimeoutMs);
  }
  if (!isAbsolute(options.dataDirectory)) {
    throw new InvalidStoreOptionsError("dataDirectory must be an absolute path");
  }

  const verifier = options.pathVerifier ?? defaultPathVerifier();
  const prepared = prepareDatabasePath(options.dataDirectory, verifier);
  const database = new Database(prepared.databasePath);
  try {
    reverifyDatabasePath(prepared, verifier);
    configure(database, false, busyTimeoutMs);
    return new SqliteEventStore(database, busyTimeoutMs);
  } catch (error: unknown) {
    database.close();
    throw error;
  }
}
