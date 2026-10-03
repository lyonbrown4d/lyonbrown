import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import Database from "better-sqlite3";
import { afterEach, describe, expect, it } from "vitest";

import {
  CorruptEventStoreError,
  PathSecurityError,
  openEventStore,
  type EventAlreadyExistsError,
  type PathSecurityVerifier,
} from "../src/index.js";
import { prepareDatabasePath } from "../src/path-security.js";

const allow: PathSecurityVerifier = {
  verifyDataDirectory: () => undefined,
  verifyDatabaseFile: () => undefined,
};
const directories: string[] = [];

function directory(): string {
  const result = mkdtempSync(join(tmpdir(), "lyonbrown-corrupt-"));
  directories.push(result);
  return result;
}

afterEach(() => {
  for (const path of directories.splice(0)) rmSync(path, { force: true, recursive: true });
});

describe("corrupt and hostile persistence", () => {
  it("fails closed when a stored payload is malformed", async () => {
    const dataDirectory = directory();
    const store = openEventStore({ dataDirectory, pathVerifier: allow });
    await store.append("session", 0n, [{ eventId: "event", type: "Type", payload: {} }]);
    store.close();

    const database = new Database(join(dataDirectory, "lyonbrown.sqlite3"));
    database.prepare("UPDATE events SET payload_json = ? WHERE event_id = ?").run("{", "event");
    database.close();

    const reopened = openEventStore({ dataDirectory, pathVerifier: allow });
    await expect(reopened.load("session", { afterRevision: 0n, limit: 1 })).rejects.toBeInstanceOf(
      CorruptEventStoreError,
    );
    reopened.close();
  });

  it("reports an event id that already exists in another session", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    await store.append("first", 0n, [{ eventId: "global", type: "Type", payload: {} }]);
    await expect(
      store.append("second", 0n, [{ eventId: "global", type: "Type", payload: {} }]),
    ).rejects.toEqual(
      expect.objectContaining<Partial<EventAlreadyExistsError>>({ eventId: "global" }),
    );
    expect(await store.currentRevision("second")).toBe(0n);
    store.close();
  });

  it("rejects future schema versions rather than guessing", () => {
    const dataDirectory = directory();
    const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
    const database = new Database(databasePath);
    database.pragma("user_version = 2");
    database.close();

    expect(() => openEventStore({ dataDirectory, pathVerifier: allow })).toThrow(
      CorruptEventStoreError,
    );
  });

  it("invokes both path-verification boundaries and propagates fail-closed results", () => {
    const dataDirectory = directory();
    const seen: string[] = [];
    const verifier: PathSecurityVerifier = {
      verifyDataDirectory: (path) => seen.push(`dir:${path}`),
      verifyDatabaseFile: (path) => seen.push(`db:${path}`),
    };
    const store = openEventStore({ dataDirectory, pathVerifier: verifier });
    store.close();
    expect(seen.filter((entry) => entry.startsWith("dir:"))).toHaveLength(1);
    expect(seen.filter((entry) => entry.startsWith("db:"))).toHaveLength(2);

    expect(() =>
      prepareDatabasePath(dataDirectory, {
        verifyDataDirectory: () => {
          throw new PathSecurityError("blocked reparse point");
        },
        verifyDatabaseFile: () => undefined,
      }),
    ).toThrow(PathSecurityError);
  });

  it("rejects operations and diagnostics after close", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    store.close();
    expect(() => store.diagnostics()).toThrow(/closed/i);
    await expect(store.listSessions({ limit: 1 })).rejects.toThrow(/closed/i);
  });
});
