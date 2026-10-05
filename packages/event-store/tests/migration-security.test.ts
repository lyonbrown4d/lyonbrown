import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import Database from "better-sqlite3";
import { afterEach, describe, expect, it } from "vitest";

import { openEventStore, type PathSecurityVerifier } from "../src/index.ts";

const allow: PathSecurityVerifier = {
  verifyRequestedDataDirectory: () => undefined,
  verifyDataDirectory: (_requested, canonical) => canonical,
  verifyDatabaseFile: (_requested, _canonical, databasePath) => databasePath,
};
const directories: string[] = [];

function directory(): string {
  const result = mkdtempSync(join(tmpdir(), "lyonbrown-migration-"));
  directories.push(result);
  return result;
}

function createVersionOne(databasePath: string, sessionId: string): void {
  const database = new Database(databasePath);
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
  database
    .prepare(
      "INSERT INTO sessions(session_id, current_revision, created_at, updated_at) VALUES (?, 0, ?, ?)",
    )
    .run(sessionId, "2026-01-01T00:00:00.000Z", "2026-01-01T00:00:00.000Z");
  database.close();
}

afterEach(() => {
  for (const path of directories.splice(0)) rmSync(path, { force: true, recursive: true });
});

describe("schema migration failure recovery", () => {
  it("preserves valid v1 sessions and events while upgrading atomically", async () => {
    const dataDirectory = directory();
    const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
    createVersionOne(databasePath, "session");
    const database = new Database(databasePath);
    database
      .prepare("UPDATE sessions SET current_revision = 1 WHERE session_id = ?")
      .run("session");
    database
      .prepare(
        "INSERT INTO events(session_id, revision, event_id, event_type, occurred_at, payload_json) VALUES (?, 1, ?, ?, ?, ?)",
      )
      .run("session", "event", "Migrated", "2026-01-01T00:00:00.000Z", '{"ok":true}');
    database.close();

    const store = openEventStore({ dataDirectory, pathVerifier: allow });
    expect(store.diagnostics().schemaVersion).toBe(2);
    expect(await store.load("session", { afterRevision: 0n, limit: 10 })).toEqual([
      expect.objectContaining({ eventId: "event", revision: 1n, payload: { ok: true } }),
    ]);
    store.close();
  });

  it("rolls back a v1 migration that encounters corrupt rows and can recover after repair", () => {
    const dataDirectory = directory();
    const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
    createVersionOne(databasePath, "x".repeat(129));

    expect(() => openEventStore({ dataDirectory, pathVerifier: allow })).toThrow();
    const inspect = new Database(databasePath);
    expect(inspect.pragma("user_version", { simple: true })).toBe(1);
    expect(
      inspect.prepare("SELECT COUNT(*) AS count FROM sessions").get() as { readonly count: number },
    ).toEqual({ count: 1 });
    inspect.prepare("UPDATE sessions SET session_id = ?").run("repaired");
    inspect.close();

    const recovered = openEventStore({ dataDirectory, pathVerifier: allow });
    expect(recovered.diagnostics().schemaVersion).toBe(2);
    recovered.close();
  });
});
