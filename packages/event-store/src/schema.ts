import { CorruptEventStoreError } from "./errors.js";

export const SCHEMA_VERSION = 2;

interface MigrationDatabase {
  exec(sql: string): void;
  pragma(source: string, options?: { readonly simple?: boolean }): unknown;
}

const CREATE_SCHEMA_V1 = `
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
`;

const CREATE_SCHEMA_V2 = `
  CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY NOT NULL
      CHECK(length(CAST(session_id AS BLOB)) BETWEEN 1 AND 128),
    current_revision INTEGER NOT NULL CHECK(current_revision >= 0),
    created_at TEXT NOT NULL CHECK(length(CAST(created_at AS BLOB)) = 24 AND substr(created_at, 24) = 'Z'),
    updated_at TEXT NOT NULL CHECK(length(CAST(updated_at AS BLOB)) = 24 AND substr(updated_at, 24) = 'Z')
  ) STRICT;
  CREATE TABLE events (
    session_id TEXT NOT NULL
      CHECK(length(CAST(session_id AS BLOB)) BETWEEN 1 AND 128),
    revision INTEGER NOT NULL CHECK(revision > 0),
    event_id TEXT NOT NULL UNIQUE
      CHECK(length(CAST(event_id AS BLOB)) BETWEEN 1 AND 128),
    event_type TEXT NOT NULL
      CHECK(length(CAST(event_type AS BLOB)) BETWEEN 1 AND 128),
    occurred_at TEXT NOT NULL
      CHECK(length(CAST(occurred_at AS BLOB)) = 24 AND substr(occurred_at, 24) = 'Z'),
    payload_json TEXT NOT NULL CHECK(length(CAST(payload_json AS BLOB)) <= 1048576),
    PRIMARY KEY(session_id, revision),
    FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE RESTRICT
  ) STRICT;
  CREATE INDEX events_session_revision ON events(session_id, revision);
`;

function migrateVersionOne(database: MigrationDatabase): void {
  database.exec(`
    ALTER TABLE events RENAME TO events_v1;
    DROP INDEX events_session_revision;
    ALTER TABLE sessions RENAME TO sessions_v1;
    ${CREATE_SCHEMA_V2}
    INSERT INTO sessions SELECT * FROM sessions_v1;
    INSERT INTO events SELECT * FROM events_v1;
    DROP TABLE events_v1;
    DROP TABLE sessions_v1;
  `);
}

interface MigrationStep {
  readonly fromVersion: number;
  readonly toVersion: number;
  apply(database: MigrationDatabase): void;
}

const MIGRATION_STEPS: readonly MigrationStep[] = [
  {
    fromVersion: 0,
    toVersion: 1,
    apply: (database) => {
      database.exec(CREATE_SCHEMA_V1);
    },
  },
  {
    fromVersion: 1,
    toVersion: 2,
    apply: migrateVersionOne,
  },
];

export function migrate(database: MigrationDatabase): void {
  let currentVersion = Number(database.pragma("user_version", { simple: true }));
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
    while (currentVersion < SCHEMA_VERSION) {
      const step = MIGRATION_STEPS.find(({ fromVersion }) => fromVersion === currentVersion);
      if (step === undefined) {
        throw new CorruptEventStoreError(
          `No migration step exists from schema ${String(currentVersion)}`,
        );
      }
      step.apply(database);
      currentVersion = step.toVersion;
      database.exec(`PRAGMA user_version = ${String(currentVersion)}`);
    }
    database.exec("COMMIT");
  } catch (error: unknown) {
    try {
      database.exec("ROLLBACK");
    } catch {
      // SQLite may already have rolled back a failed COMMIT; preserve the first failure.
    }
    throw error;
  }
}
