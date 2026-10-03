import Database from "better-sqlite3";
import { Kysely, SqliteDialect } from "kysely";
import { isAbsolute } from "node:path";

import { decodeEventPayload, encodeEventPayload } from "./codec.js";
import {
  CorruptEventStoreError,
  EventAlreadyExistsError,
  EventStoreClosedError,
  InvalidStoreOptionsError,
  RevisionConflictError,
} from "./errors.js";
import { defaultPathVerifier, prepareDatabasePath } from "./path-security.js";
import { migrate, SCHEMA_VERSION, type EventRow, type EventStoreDatabase } from "./schema.js";
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
import {
  MAX_BATCH_PAYLOAD_BYTES,
  validateAppend,
  validateList,
  validateLoad,
} from "./validation.js";

interface RevisionRow {
  readonly current_revision: bigint;
}

function isUniqueConstraint(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof error.code === "string" &&
    error.code.startsWith("SQLITE_CONSTRAINT_UNIQUE")
  );
}

function mapStoredEvent(row: EventRow): StoredEvent {
  try {
    return {
      eventId: row.event_id,
      sessionId: row.session_id,
      revision: row.revision,
      type: row.event_type,
      occurredAt: row.occurred_at,
      payload: decodeEventPayload(row.payload_json),
    };
  } catch (error: unknown) {
    throw new CorruptEventStoreError(
      `Event ${JSON.stringify(row.event_id)} contains an invalid payload`,
      { cause: error },
    );
  }
}

class SqliteEventStore implements EventStore {
  readonly #database: Database.Database;
  readonly #kysely: Kysely<EventStoreDatabase>;
  #closed = false;

  public constructor(database: Database.Database) {
    this.#database = database;
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
    validateAppend(sessionId, expectedRevision, events);
    const now = new Date().toISOString();
    const encoded: {
      readonly event: EventToAppend;
      readonly occurredAt: string;
      readonly payloadJson: string;
    }[] = [];
    let batchBytes = 0;
    for (const event of events) {
      const payloadJson = encodeEventPayload(event.payload);
      batchBytes += Buffer.byteLength(payloadJson, "utf8");
      if (batchBytes > MAX_BATCH_PAYLOAD_BYTES) {
        throw new RangeError("Encoded event batch exceeds the 4 MiB payload budget");
      }
      encoded.push({ event, occurredAt: event.occurredAt ?? now, payloadJson });
    }

    const findRevision = this.#database.prepare<[string], RevisionRow>(
      "SELECT current_revision FROM sessions WHERE session_id = ?",
    );
    const findEvent = this.#database.prepare<[string], { readonly event_id: string }>(
      "SELECT event_id FROM events WHERE event_id = ?",
    );
    const insertSession = this.#database.prepare(
      "INSERT INTO sessions(session_id, current_revision, created_at, updated_at) VALUES (?, ?, ?, ?)",
    );
    const updateSession = this.#database.prepare(
      "UPDATE sessions SET current_revision = ?, updated_at = ? WHERE session_id = ?",
    );
    const insertEvent = this.#database.prepare(
      "INSERT INTO events(session_id, revision, event_id, event_type, occurred_at, payload_json) VALUES (?, ?, ?, ?, ?, ?)",
    );

    const transaction = this.#database.transaction((): readonly StoredEvent[] => {
      const revisionRow = findRevision.get(sessionId);
      const actualRevision = revisionRow?.current_revision ?? 0n;
      if (actualRevision !== expectedRevision) {
        throw new RevisionConflictError(sessionId, expectedRevision, actualRevision);
      }
      for (const item of encoded) {
        if (findEvent.get(item.event.eventId) !== undefined) {
          throw new EventAlreadyExistsError(item.event.eventId);
        }
      }
      if (revisionRow === undefined) insertSession.run(sessionId, 0n, now, now);

      const stored: StoredEvent[] = [];
      let revision = actualRevision;
      for (const item of encoded) {
        revision += 1n;
        insertEvent.run(
          sessionId,
          revision,
          item.event.eventId,
          item.event.type,
          item.occurredAt,
          item.payloadJson,
        );
        stored.push({
          eventId: item.event.eventId,
          sessionId,
          revision,
          type: item.event.type,
          occurredAt: item.occurredAt,
          payload: decodeEventPayload(item.payloadJson),
        });
      }
      updateSession.run(revision, now, sessionId);
      return stored;
    });

    try {
      return transaction.immediate();
    } catch (error: unknown) {
      if (error instanceof RevisionConflictError || error instanceof EventAlreadyExistsError)
        throw error;
      if (isUniqueConstraint(error)) {
        const duplicate = encoded.find((item) => findEvent.get(item.event.eventId) !== undefined);
        throw new EventAlreadyExistsError(duplicate?.event.eventId ?? "unknown", { cause: error });
      }
      throw error;
    }
  }

  public async load(sessionId: string, options: LoadOptions): Promise<readonly StoredEvent[]> {
    this.#assertOpen();
    validateLoad(sessionId, options);
    const rows = await this.#kysely
      .selectFrom("events")
      .selectAll()
      .where("session_id", "=", sessionId)
      .where("revision", ">", options.afterRevision)
      .orderBy("revision", "asc")
      .limit(options.limit)
      .execute();
    return rows.map(mapStoredEvent);
  }

  public async currentRevision(sessionId: string): Promise<bigint> {
    this.#assertOpen();
    validateLoad(sessionId, { afterRevision: 0n, limit: 1 });
    const row = await this.#kysely
      .selectFrom("sessions")
      .select("current_revision")
      .where("session_id", "=", sessionId)
      .executeTakeFirst();
    return row?.current_revision ?? 0n;
  }

  public async listSessions(options: ListSessionsOptions): Promise<readonly SessionMetadata[]> {
    this.#assertOpen();
    validateList(options);
    const rows = await this.#kysely
      .selectFrom("sessions")
      .selectAll()
      .orderBy("updated_at", "desc")
      .orderBy("session_id", "asc")
      .limit(options.limit)
      .offset(options.offset ?? 0)
      .execute();
    return rows.map((row) => ({
      sessionId: row.session_id,
      currentRevision: row.current_revision,
      createdAt: row.created_at,
      updatedAt: row.updated_at,
    }));
  }

  public diagnostics(): StoreDiagnostics {
    this.#assertOpen();
    return {
      journalMode: String(this.#database.pragma("journal_mode", { simple: true })),
      schemaVersion: Number(this.#database.pragma("user_version", { simple: true })),
      synchronous: Number(this.#database.pragma("synchronous", { simple: true })),
    };
  }

  public close(): void {
    if (this.#closed) return;
    this.#closed = true;
    this.#database.close();
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
    return new SqliteEventStore(database);
  }

  if (!isAbsolute(options.dataDirectory)) {
    throw new InvalidStoreOptionsError("dataDirectory must be an absolute path");
  }
  const verifier = options.pathVerifier ?? defaultPathVerifier();
  const { databasePath } = prepareDatabasePath(options.dataDirectory, verifier);
  const database = new Database(databasePath);
  try {
    verifier.verifyDatabaseFile(databasePath);
    configure(database, false, busyTimeoutMs);
    return new SqliteEventStore(database);
  } catch (error: unknown) {
    database.close();
    throw error;
  }
}
