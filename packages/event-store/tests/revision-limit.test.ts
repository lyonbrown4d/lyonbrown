import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import Database from "better-sqlite3";
import { afterEach, describe, expect, it } from "vitest";

import { RevisionExhaustedError, openEventStore, type PathSecurityVerifier } from "../src/index.js";

const allow: PathSecurityVerifier = {
  verifyRequestedDataDirectory: () => undefined,
  verifyDataDirectory: (_requested, canonical) => canonical,
  verifyDatabaseFile: (_requested, _canonical, databasePath) => databasePath,
};
const directories: string[] = [];

afterEach(() => {
  for (const path of directories.splice(0)) rmSync(path, { force: true, recursive: true });
});

describe("signed 64-bit revision exhaustion", () => {
  it("commits INT64_MAX and returns a typed exhaustion error for the next event", async () => {
    const dataDirectory = mkdtempSync(join(tmpdir(), "lyonbrown-max-revision-"));
    directories.push(dataDirectory);
    const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
    const initial = openEventStore({ dataDirectory, pathVerifier: allow });
    initial.close();

    const database = new Database(databasePath);
    database
      .prepare(
        "INSERT INTO sessions(session_id, current_revision, created_at, updated_at) VALUES (?, ?, ?, ?)",
      )
      .run(
        "session",
        9_223_372_036_854_775_806n,
        "2026-01-01T00:00:00.000Z",
        "2026-01-01T00:00:00.000Z",
      );
    database
      .prepare(
        "INSERT INTO events(session_id, revision, event_id, event_type, occurred_at, payload_json) VALUES (?, ?, ?, ?, ?, ?)",
      )
      .run(
        "session",
        9_223_372_036_854_775_806n,
        "event-before-max",
        "BeforeMax",
        "2026-01-01T00:00:00.000Z",
        "{}",
      );
    database.close();

    const store = openEventStore({ dataDirectory, pathVerifier: allow });
    const committed = await store.append("session", 9_223_372_036_854_775_806n, [
      { eventId: "event-max", type: "Max", payload: {} },
    ]);
    expect(committed[0]?.revision).toBe(9_223_372_036_854_775_807n);
    await expect(
      store.append("session", 9_223_372_036_854_775_807n, [
        { eventId: "event-overflow", type: "Overflow", payload: {} },
      ]),
    ).rejects.toBeInstanceOf(RevisionExhaustedError);
    store.close();
  });
});
