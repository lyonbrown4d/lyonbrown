import { chmodSync } from "node:fs";
import { isAbsolute } from "node:path";

import Database from "better-sqlite3";

import { appendEvents } from "@lyonbrown/event-store/internal/append";
import {
  CorruptEventStoreError,
  EventStoreClosedError,
  EventStoreError,
  InvalidStoreOptionsError,
  PathSecurityError,
} from "@lyonbrown/event-store/internal/errors";
import {
  validateEventRows,
  validateSessionIntegrity,
  type RuntimeEventRow,
  type SessionIntegrityRow,
} from "@lyonbrown/event-store/internal/integrity";
import {
  defaultPathVerifier,
  prepareDatabasePath,
  reverifyDatabasePath,
  verifyDatabaseSidecars,
} from "@lyonbrown/event-store/internal/path-security";
import { migrate, SCHEMA_VERSION } from "@lyonbrown/event-store/internal/schema";
import type {
  EventStore,
  EventToAppend,
  ListSessionsOptions,
  LoadOptions,
  OpenEventStoreOptions,
  PathSecurityVerifier,
  SessionMetadata,
  StoredEvent,
  StoreDiagnostics,
} from "@lyonbrown/event-store/internal/types";
import { validateList, validateLoad } from "@lyonbrown/event-store/internal/validation";

const SESSION_INTEGRITY_SELECT = `
  SELECT s.session_id, s.current_revision, s.created_at, s.updated_at,
         COALESCE(MAX(e.revision), 0) AS max_revision,
         COUNT(e.revision) AS event_count
    FROM sessions s
    LEFT JOIN events e ON e.session_id = s.session_id
`;

function closeStoreResources(
  database: Database.Database | undefined,
  verifier: PathSecurityVerifier | undefined,
): EventStoreError | undefined {
  let failure: EventStoreError | undefined;
  try {
    database?.close();
  } catch {
    failure = new EventStoreError("Event store close failed");
  }
  try {
    verifier?.close?.();
  } catch {
    failure ??= new PathSecurityError("Secure path verifier cleanup failed");
  }
  return failure;
}

function redactOpenFailure(error: unknown): EventStoreError {
  return error instanceof EventStoreError ? error : new EventStoreError("Event store open failed");
}

class SqliteEventStore implements EventStore {
  readonly #database: Database.Database;
  readonly #busyTimeoutMs: number;
  readonly #verifier: PathSecurityVerifier | undefined;
  #closed = false;

  public constructor(
    database: Database.Database,
    busyTimeoutMs: number,
    verifier?: PathSecurityVerifier,
  ) {
    this.#database = database;
    this.#busyTimeoutMs = busyTimeoutMs;
    this.#verifier = verifier;
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

  // All native work completes synchronously before the returned Promise can be observed.
  // eslint-disable-next-line @typescript-eslint/require-await
  public async load(sessionId: string, options: LoadOptions): Promise<readonly StoredEvent[]> {
    this.#assertOpen();
    validateLoad(sessionId, options);
    const metadata = this.#sessionIntegrity(sessionId);
    if (metadata === undefined) return [];
    const rows = this.#database
      .prepare<[string, bigint, number], RuntimeEventRow>(
        `SELECT session_id, revision, event_id, event_type, occurred_at, payload_json
           FROM events
          WHERE session_id = ? AND revision > ?
          ORDER BY revision ASC
          LIMIT ?`,
      )
      .all(sessionId, options.afterRevision, options.limit);
    if (options.afterRevision < metadata.currentRevision && rows.length === 0) {
      throw new CorruptEventStoreError("Event stream ended before its recorded current revision");
    }
    return validateEventRows(rows, sessionId, options.afterRevision);
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
    const failure = closeStoreResources(this.#database, this.#verifier);
    if (failure) throw failure;
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
  let database: Database.Database | undefined;
  try {
    const prepared = prepareDatabasePath(options.dataDirectory, verifier);
    database = new Database(prepared.databasePath);
    if (process.platform !== "win32" && prepared.databaseIdentity === undefined) {
      chmodSync(prepared.databasePath, 0o600);
    }
    reverifyDatabasePath(prepared, verifier);
    configure(database, false, busyTimeoutMs);
    verifyDatabaseSidecars(prepared, verifier);
    return new SqliteEventStore(database, busyTimeoutMs, verifier);
  } catch (error: unknown) {
    closeStoreResources(database, verifier);
    throw redactOpenFailure(error);
  }
}
