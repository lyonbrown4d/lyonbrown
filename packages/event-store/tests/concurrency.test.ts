import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Worker } from "node:worker_threads";

import { afterEach, describe, expect, it } from "vitest";

import { StorageBusyError, openEventStore, type PathSecurityVerifier } from "../src/index.js";

const allow: PathSecurityVerifier = {
  verifyRequestedDataDirectory: () => undefined,
  verifyDataDirectory: (_requested, canonical) => canonical,
  verifyDatabaseFile: (_requested, _canonical, databasePath) => databasePath,
};
const directories: string[] = [];

function directory(): string {
  const result = mkdtempSync(join(tmpdir(), "lyonbrown-lock-"));
  directories.push(result);
  return result;
}

function holdWriteLock(databasePath: string, holdMilliseconds: number): Promise<Worker> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(
      `
        const { parentPort, workerData } = require("node:worker_threads");
        const Database = require("better-sqlite3");
        const database = new Database(workerData.databasePath);
        database.exec("BEGIN IMMEDIATE");
        parentPort.postMessage("locked");
        setTimeout(() => {
          database.exec("COMMIT");
          database.close();
          parentPort.postMessage("released");
        }, workerData.holdMilliseconds);
      `,
      { eval: true, workerData: { databasePath, holdMilliseconds } },
    );
    worker.once("message", () => {
      resolve(worker);
    });
    worker.once("error", reject);
  });
}

function released(worker: Worker): Promise<void> {
  return new Promise((resolve, reject) => {
    worker.once("message", () => {
      resolve();
    });
    worker.once("error", reject);
  });
}

afterEach(() => {
  for (const path of directories.splice(0)) rmSync(path, { force: true, recursive: true });
});

describe("two-connection locking", () => {
  it("waits for a concurrent writer and then commits within busy_timeout", async () => {
    const dataDirectory = directory();
    const store = openEventStore({ dataDirectory, pathVerifier: allow, busyTimeoutMs: 1000 });
    const worker = await holdWriteLock(join(dataDirectory, "lyonbrown.sqlite3"), 120);
    const started = performance.now();
    await store.append("session", 0n, [{ eventId: "event", type: "Type", payload: {} }]);
    const elapsed = performance.now() - started;

    expect(elapsed).toBeGreaterThanOrEqual(80);
    await released(worker);
    store.close();
  });

  it("returns a typed busy error after the configured lock timeout", async () => {
    const dataDirectory = directory();
    const store = openEventStore({ dataDirectory, pathVerifier: allow, busyTimeoutMs: 25 });
    const worker = await holdWriteLock(join(dataDirectory, "lyonbrown.sqlite3"), 150);
    const started = performance.now();

    await expect(
      store.append("session", 0n, [{ eventId: "event", type: "Type", payload: {} }]),
    ).rejects.toBeInstanceOf(StorageBusyError);
    expect(performance.now() - started).toBeGreaterThanOrEqual(20);
    await released(worker);
    store.close();
  });

  it("reports WAL, FULL synchronization and the configured busy timeout", () => {
    const store = openEventStore({
      dataDirectory: directory(),
      pathVerifier: allow,
      busyTimeoutMs: 4321,
    });
    expect(store.diagnostics()).toMatchObject({
      journalMode: "wal",
      synchronous: 2,
      busyTimeoutMs: 4321,
    });
    store.close();
  });
});
