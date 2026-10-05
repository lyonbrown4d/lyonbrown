import { existsSync, lstatSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import Database from "better-sqlite3";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  EventStoreError,
  PathSecurityError,
  openEventStore,
  type PathSecurityVerifier,
} from "../src/index.ts";

const directories: string[] = [];
const databaseClose = Object.getOwnPropertyDescriptor(Database.prototype, "close")?.value as (
  this: Database.Database,
) => Database.Database;

function closeDatabase(database: Database.Database): Database.Database {
  return Reflect.apply(databaseClose, database, []);
}

function temporaryDirectory(label: string): string {
  const result = mkdtempSync(join(tmpdir(), `lyonbrown-${label}-`));
  directories.push(result);
  return result;
}

function identity(path: string): string | undefined {
  if (!existsSync(path)) return undefined;
  const stats = lstatSync(path, { bigint: true });
  return `${stats.dev.toString()}:${stats.ino.toString()}`;
}

function allowingVerifier(): PathSecurityVerifier {
  return {
    verifyRequestedDataDirectory: () => undefined,
    verifyDataDirectory: (_requested, canonical) => identity(canonical) ?? "missing-directory",
    verifyDatabaseFile: (_requested, _canonical, database) => identity(database),
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  for (const directory of directories.splice(0)) {
    rmSync(directory, { force: true, recursive: true });
  }
});

describe("event store resource cleanup", () => {
  it("keeps verifier anchors until close and releases them exactly once", () => {
    const dataDirectory = temporaryDirectory("verifier-lifecycle");
    let closed = 0;
    const verifier = allowingVerifier();
    verifier.close = () => {
      closed += 1;
    };
    const store = openEventStore({ dataDirectory, pathVerifier: verifier });

    store.close();
    store.close();

    expect(closed).toBe(1);
  });

  it("closes SQLite before releasing verifier anchors", () => {
    const dataDirectory = temporaryDirectory("close-order");
    const calls: string[] = [];
    vi.spyOn(Database.prototype, "close").mockImplementation(function (this: Database.Database) {
      calls.push("database");
      return closeDatabase(this);
    });
    const verifier = allowingVerifier();
    verifier.close = () => {
      calls.push("verifier");
    };
    const store = openEventStore({ dataDirectory, pathVerifier: verifier });

    store.close();

    expect(calls).toEqual(["database", "verifier"]);
  });

  it("releases verifier anchors after a partially initialized open fails", () => {
    const dataDirectory = temporaryDirectory("verifier-open-failure");
    const calls: string[] = [];
    vi.spyOn(Database.prototype, "close").mockImplementation(function (this: Database.Database) {
      calls.push("database");
      return closeDatabase(this);
    });
    const verifier = allowingVerifier();
    verifier.close = () => {
      calls.push("verifier");
    };
    verifier.verifyDatabaseFile = (_requested, _canonical, databasePath) => {
      if (existsSync(databasePath)) throw new Error("native secret");
      return undefined;
    };

    expect(() => openEventStore({ dataDirectory, pathVerifier: verifier })).toThrow();
    expect(calls).toEqual(["database", "verifier"]);
  });

  it("returns the redacted first cleanup failure and still runs the second cleanup", () => {
    const dataDirectory = temporaryDirectory("cleanup-double-failure");
    const calls: string[] = [];
    const verifier = allowingVerifier();
    verifier.close = () => {
      calls.push("verifier");
      throw new Error("verifier cleanup secret");
    };
    const store = openEventStore({ dataDirectory, pathVerifier: verifier });
    vi.spyOn(Database.prototype, "close").mockImplementation(function (this: Database.Database) {
      calls.push("database");
      closeDatabase(this);
      throw new Error("database cleanup secret");
    });

    expect(() => {
      store.close();
    }).toThrow(new EventStoreError("Event store close failed"));
    expect(calls).toEqual(["database", "verifier"]);
  });

  it("redacts an open failure while both cleanup phases still fail in order", () => {
    const dataDirectory = temporaryDirectory("open-double-failure");
    const calls: string[] = [];
    const verifier = allowingVerifier();
    verifier.verifyDatabaseFile = (_requested, _canonical, databasePath) => {
      if (existsSync(databasePath)) throw new Error("open native secret");
      return undefined;
    };
    verifier.close = () => {
      calls.push("verifier");
      throw new Error("verifier cleanup secret");
    };
    vi.spyOn(Database.prototype, "close").mockImplementation(function (this: Database.Database) {
      calls.push("database");
      closeDatabase(this);
      throw new Error("database cleanup secret");
    });

    expect(() => openEventStore({ dataDirectory, pathVerifier: verifier })).toThrow(
      new EventStoreError("Event store open failed"),
    );
    expect(calls).toEqual(["database", "verifier"]);
  });

  it("redacts verifier cleanup failures and keeps close idempotent", () => {
    const dataDirectory = temporaryDirectory("verifier-close-failure");
    let closed = 0;
    const verifier = allowingVerifier();
    verifier.close = () => {
      closed += 1;
      throw new Error("native cleanup secret");
    };
    const databaseCloseSpy = vi.spyOn(Database.prototype, "close");
    const store = openEventStore({ dataDirectory, pathVerifier: verifier });

    expect(() => {
      store.close();
    }).toThrow(new PathSecurityError("Secure path verifier cleanup failed"));
    expect(() => {
      store.close();
    }).not.toThrow();
    expect(closed).toBe(1);
    expect(databaseCloseSpy).toHaveBeenCalledOnce();
  });
});
