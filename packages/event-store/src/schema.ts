import { CorruptEventStoreError } from "./errors.js";

export const SCHEMA_VERSION = 1;

export interface SessionRow {
  session_id: string;
  current_revision: bigint;
  created_at: string;
  updated_at: string;
}

export interface EventRow {
  session_id: string;
  revision: bigint;
  event_id: string;
  event_type: string;
  occurred_at: string;
  payload_json: string;
}

export interface EventStoreDatabase {
  sessions: SessionRow;
  events: EventRow;
}

interface MigrationDatabase {
  exec(sql: string): void;
  pragma(source: string, options?: { readonly simple?: boolean }): unknown;
}

export function migrate(database: MigrationDatabase): void {
  const currentVersion = Number(database.pragma("user_version", { simple: true }));
  if (!Number.isSafeInteger(currentVersion) || currentVersion < 0) {
    throw new CorruptEventStoreError("SQLite user_version is invalid");
  }
  if (currentVersion > SCHEMA_VERSION) {
    throw new CorruptEventStoreError(
      `Database schema ${String(currentVersion)} is newer than supported schema ${String(SCHEMA_VERSION)}`,
    );
  }
  if (currentVersion === SCHEMA_VERSION) return;

  database.exec("BEGIN IMMEDIATE");
  try {
    database.exec(`
      CREATE TABLE sessions (
        session_id TEXT PRIMARY KEY NOT NULL,
        current_revision INTEGER NOT NULL CHECK(current_revision >= 0),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
      ) STRICT;
      CREATE TABLE events (
        session_id TEXT NOT NULL,
        revision INTEGER NOT NULL CHECK(revision > 0),
        event_id TEXT NOT NULL UNIQUE,
        event_type TEXT NOT NULL,
        occurred_at TEXT NOT NULL,
        payload_json TEXT NOT NULL CHECK(length(CAST(payload_json AS BLOB)) <= 1048576),
        PRIMARY KEY(session_id, revision),
        FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE RESTRICT
      ) STRICT;
      CREATE INDEX events_session_revision ON events(session_id, revision);
      PRAGMA user_version = 1;
    `);
    database.exec("COMMIT");
  } catch (error: unknown) {
    try {
      database.exec("ROLLBACK");
    } catch {
      // Preserve the migration failure; SQLite already rolled back if COMMIT failed.
    }
    throw error;
  }
}
