import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import Database from "better-sqlite3";
import { afterEach, describe, expect, it } from "vitest";

import { CorruptEventStoreError, openEventStore, type PathSecurityVerifier } from "../src/index.js";

const allow: PathSecurityVerifier = {
  verifyRequestedDataDirectory: () => undefined,
  verifyDataDirectory: (_requested, canonical) => canonical,
  verifyDatabaseFile: (_requested, _canonical, databasePath) =>
    databasePath.endsWith("lyonbrown.sqlite3") ? databasePath : undefined,
};
const directories: string[] = [];

function directory(): string {
  const result = mkdtempSync(join(tmpdir(), "lyonbrown-integrity-"));
  directories.push(result);
  return result;
}

async function seededStore(dataDirectory: string): Promise<void> {
  const store = openEventStore({ dataDirectory, pathVerifier: allow });
  await store.append("session", 0n, [
    { eventId: "event-1", type: "One", payload: {} },
    { eventId: "event-2", type: "Two", payload: {} },
  ]);
  store.close();
}

afterEach(() => {
  for (const path of directories.splice(0)) rmSync(path, { force: true, recursive: true });
});

describe("read-side integrity validation", () => {
  it.each([
    ["oversized event id", "UPDATE events SET event_id = ? WHERE revision = 1", "x".repeat(129)],
    ["invalid timestamp", "UPDATE events SET occurred_at = ? WHERE revision = 1", "yesterday"],
    ["revision gap", "UPDATE events SET revision = ? WHERE revision = 2", 3],
  ] as const)("fails closed on %s", async (_label, sql, value) => {
    const dataDirectory = directory();
    await seededStore(dataDirectory);
    const database = new Database(join(dataDirectory, "lyonbrown.sqlite3"));
    database.pragma("ignore_check_constraints = ON");
    database.prepare(sql).run(value);
    database.close();

    const store = openEventStore({ dataDirectory, pathVerifier: allow });
    await expect(store.load("session", { afterRevision: 0n, limit: 10 })).rejects.toBeInstanceOf(
      CorruptEventStoreError,
    );
    store.close();
  });

  it("detects sessions.current_revision disagreement with the event stream", async () => {
    const dataDirectory = directory();
    await seededStore(dataDirectory);
    const database = new Database(join(dataDirectory, "lyonbrown.sqlite3"));
    database
      .prepare("UPDATE sessions SET current_revision = 9 WHERE session_id = ?")
      .run("session");
    database.close();

    const store = openEventStore({ dataDirectory, pathVerifier: allow });
    await expect(store.currentRevision("session")).rejects.toBeInstanceOf(CorruptEventStoreError);
    await expect(store.listSessions({ limit: 10 })).rejects.toBeInstanceOf(CorruptEventStoreError);
    store.close();
  });

  it("rejects corrupt session identifiers and metadata while listing", async () => {
    const dataDirectory = directory();
    await seededStore(dataDirectory);
    const database = new Database(join(dataDirectory, "lyonbrown.sqlite3"));
    database.pragma("foreign_keys = OFF");
    database.pragma("ignore_check_constraints = ON");
    const invalid = "x".repeat(129);
    database.prepare("UPDATE sessions SET session_id = ?, updated_at = ?").run(invalid, "invalid");
    database.prepare("UPDATE events SET session_id = ?").run(invalid);
    database.close();

    const store = openEventStore({ dataDirectory, pathVerifier: allow });
    await expect(store.listSessions({ limit: 10 })).rejects.toBeInstanceOf(CorruptEventStoreError);
    store.close();
  });

  it("detects event rows moved to a different session", async () => {
    const dataDirectory = directory();
    await seededStore(dataDirectory);
    const database = new Database(join(dataDirectory, "lyonbrown.sqlite3"));
    database.pragma("foreign_keys = OFF");
    database.prepare("UPDATE events SET session_id = ? WHERE revision = 2").run("other");
    database.close();

    const store = openEventStore({ dataDirectory, pathVerifier: allow });
    await expect(store.load("session", { afterRevision: 0n, limit: 10 })).rejects.toBeInstanceOf(
      CorruptEventStoreError,
    );
    store.close();
  });
});
