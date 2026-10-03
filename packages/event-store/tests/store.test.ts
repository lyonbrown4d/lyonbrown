import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import Database from "better-sqlite3";
import { afterEach, describe, expect, it } from "vitest";

import {
  CorruptEventStoreError,
  EventAlreadyExistsError,
  EventStoreClosedError,
  InvalidStoreOptionsError,
  PathSecurityError,
  openEventStore,
  type PathSecurityVerifier,
  type RevisionConflictError,
} from "../src/index.js";

const allowTestPath: PathSecurityVerifier = {
  verifyRequestedDataDirectory: () => undefined,
  verifyDataDirectory: (_requested, canonical) => canonical,
  verifyDatabaseFile: (_requested, _canonical, databasePath) => databasePath,
};

const temporaryDirectories: string[] = [];

function makeDataDirectory(): string {
  const directory = mkdtempSync(join(tmpdir(), "lyonbrown-event-store-"));
  temporaryDirectories.push(directory);
  return directory;
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { force: true, recursive: true });
  }
});

describe("SQLite event store", () => {
  it("appends atomically with monotonic bigint revisions and stable reads", async () => {
    const store = openEventStore({
      dataDirectory: makeDataDirectory(),
      pathVerifier: allowTestPath,
    });

    const appended = await store.append("session-a", 0n, [
      { eventId: "event-1", type: "SessionStarted", payload: { title: "Hello" } },
      { eventId: "event-2", type: "UserMessage", payload: { text: "world" } },
    ]);

    expect(appended.map((event) => event.revision)).toEqual([1n, 2n]);
    expect(await store.currentRevision("session-a")).toBe(2n);
    expect((await store.load("session-a", { afterRevision: 0n, limit: 1 }))[0]?.eventId).toBe(
      "event-1",
    );
    expect((await store.load("session-a", { afterRevision: 1n, limit: 10 }))[0]?.eventId).toBe(
      "event-2",
    );
    expect((await store.listSessions({ limit: 10 }))[0]).toMatchObject({
      sessionId: "session-a",
      currentRevision: 2n,
    });
    store.close();
  });

  it("returns the validated persisted payload instead of retaining the caller object", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const payload = { text: "persisted" };
    const committed = await store.append("session-a", 0n, [
      { eventId: "event-1", type: "Message", payload },
    ]);
    payload.text = "mutated later";

    expect(committed[0]?.payload).toEqual({ text: "persisted" });
    expect(committed[0]?.payload).not.toBe(payload);
    store.close();
  });

  it("returns a typed optimistic concurrency conflict without a partial write", async () => {
    const store = openEventStore({
      dataDirectory: makeDataDirectory(),
      pathVerifier: allowTestPath,
    });
    await store.append("session-a", 0n, [
      { eventId: "event-1", type: "SessionStarted", payload: {} },
    ]);

    await expect(
      store.append("session-a", 0n, [
        { eventId: "event-2", type: "UserMessage", payload: { text: "lost" } },
      ]),
    ).rejects.toEqual(
      expect.objectContaining<Partial<RevisionConflictError>>({ actualRevision: 1n }),
    );
    expect(await store.currentRevision("session-a")).toBe(1n);
    store.close();
  });

  it("rejects duplicate event ids and rolls back the entire batch", async () => {
    const store = openEventStore({
      dataDirectory: makeDataDirectory(),
      pathVerifier: allowTestPath,
    });

    await expect(
      store.append("session-a", 0n, [
        { eventId: "same", type: "First", payload: {} },
        { eventId: "same", type: "Second", payload: {} },
      ]),
    ).rejects.toBeInstanceOf(EventAlreadyExistsError);
    expect(await store.currentRevision("session-a")).toBe(0n);
    expect(await store.load("session-a", { afterRevision: 0n, limit: 10 })).toEqual([]);
    store.close();
  });

  it.each([
    ["same session exact retry", "session-a", 0n],
    ["other session with stale expected revision", "session-b", 99n],
  ] as const)("prioritizes global event identity for %s", async (_label, sessionId, expected) => {
    const store = openEventStore({ testOnlyMemory: true });
    await store.append("session-a", 0n, [
      { eventId: "event-global", type: "Original", payload: { stable: true } },
    ]);

    await expect(
      store.append(sessionId, expected, [
        { eventId: "event-global", type: "Retry", payload: { stable: true } },
      ]),
    ).rejects.toEqual(
      expect.objectContaining<Partial<EventAlreadyExistsError>>({ eventId: "event-global" }),
    );
    store.close();
  });

  it("serializes concurrent appends so only one expected revision wins", async () => {
    const store = openEventStore({
      dataDirectory: makeDataDirectory(),
      pathVerifier: allowTestPath,
    });
    const attempts = await Promise.allSettled([
      store.append("session-a", 0n, [{ eventId: "event-a", type: "A", payload: {} }]),
      store.append("session-a", 0n, [{ eventId: "event-b", type: "B", payload: {} }]),
    ]);

    expect(attempts.filter((attempt) => attempt.status === "fulfilled")).toHaveLength(1);
    expect(attempts.filter((attempt) => attempt.status === "rejected")).toHaveLength(1);
    expect(await store.currentRevision("session-a")).toBe(1n);
    store.close();
  });

  it("persists in WAL mode, migrates idempotently and reopens", async () => {
    const dataDirectory = makeDataDirectory();
    const first = openEventStore({ dataDirectory, pathVerifier: allowTestPath });
    await first.append("session-a", 0n, [
      { eventId: "event-1", type: "SessionStarted", payload: { durable: true } },
    ]);
    expect(first.diagnostics().journalMode).toBe("wal");
    expect(first.diagnostics().schemaVersion).toBe(2);
    first.close();

    const reopened = openEventStore({ dataDirectory, pathVerifier: allowTestPath });
    expect(await reopened.currentRevision("session-a")).toBe(1n);
    expect(reopened.diagnostics().schemaVersion).toBe(2);
    reopened.close();
  });

  it("preserves revisions above Number.MAX_SAFE_INTEGER", async () => {
    const dataDirectory = makeDataDirectory();
    const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
    const initial = openEventStore({ dataDirectory, pathVerifier: allowTestPath });
    initial.close();

    const seed = new Database(databasePath);
    seed.pragma("safe_integers = ON");
    seed
      .prepare(
        "INSERT INTO sessions(session_id, current_revision, created_at, updated_at) VALUES (?, ?, ?, ?)",
      )
      .run("huge", 9_007_199_254_740_992n, "2026-01-01T00:00:00.000Z", "2026-01-01T00:00:00.000Z");
    seed
      .prepare(
        "INSERT INTO events(session_id, revision, event_id, event_type, occurred_at, payload_json) VALUES (?, ?, ?, ?, ?, ?)",
      )
      .run(
        "huge",
        9_007_199_254_740_992n,
        "event-before-huge",
        "BeforeHuge",
        "2026-01-01T00:00:00.000Z",
        "{}",
      );
    seed.close();

    const store = openEventStore({ dataDirectory, pathVerifier: allowTestPath });
    const appended = await store.append("huge", 9_007_199_254_740_992n, [
      { eventId: "event-huge", type: "Huge", payload: {} },
    ]);
    expect(appended[0]?.revision).toBe(9_007_199_254_740_993n);
    await expect(store.currentRevision("huge")).rejects.toBeInstanceOf(CorruptEventStoreError);
    store.close();
  });

  it("parameterizes hostile session ids instead of executing them", async () => {
    const store = openEventStore({
      dataDirectory: makeDataDirectory(),
      pathVerifier: allowTestPath,
    });
    const hostile = "x'; DROP TABLE events; --";
    await store.append(hostile, 0n, [{ eventId: "event-1", type: "Safe", payload: {} }]);
    await store.append("still-here", 0n, [{ eventId: "event-2", type: "Safe", payload: {} }]);

    expect(await store.currentRevision(hostile)).toBe(1n);
    expect(await store.currentRevision("still-here")).toBe(1n);
    store.close();
  });

  it("fails closed for non-canonical paths and Windows without a verifier", () => {
    expect(() => openEventStore({ dataDirectory: "relative" })).toThrow(InvalidStoreOptionsError);
    if (process.platform === "win32") {
      expect(() => openEventStore({ dataDirectory: makeDataDirectory() })).toThrow(
        PathSecurityError,
      );
    }
  });

  it("supports explicit test-only memory stores and idempotent close", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    await store.append("session-a", 0n, [{ eventId: "event-1", type: "Safe", payload: {} }]);
    store.close();
    store.close();
    await expect(store.currentRevision("session-a")).rejects.toBeInstanceOf(EventStoreClosedError);
  });
});
