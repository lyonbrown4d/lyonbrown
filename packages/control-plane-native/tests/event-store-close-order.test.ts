import { existsSync, mkdirSync, mkdtempSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, win32 } from "node:path";

import Database from "better-sqlite3";
import {
  openEventStore,
  PathSecurityError,
  type PathSecurityVerifier,
} from "@lyonbrown/event-store";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createNativeEventStorePathVerifier } from "../src/index.ts";
import { api, type Handle } from "../src/windows/api.ts";

const temporaryDirectories: string[] = [];
const databaseClose = Object.getOwnPropertyDescriptor(Database.prototype, "close")?.value as (
  this: Database.Database,
) => Database.Database;

function closeDatabase(database: Database.Database): Database.Database {
  return Reflect.apply(databaseClose, database, []);
}

function temporaryDirectory(label: string): string {
  const path = mkdtempSync(join(tmpdir(), `lyonbrown-native-close-${label}-`));
  temporaryDirectories.push(path);
  return path;
}

function nativeHandlePath(handle: Handle): string {
  const required = Number(api.GetFinalPathNameByHandleW(handle, null, 0, 0));
  const output = Buffer.alloc((required + 1) * 2);
  const written = Number(api.GetFinalPathNameByHandleW(handle, output, required + 1, 0));
  return output
    .subarray(0, written * 2)
    .toString("utf16le")
    .replace(/^\\\\\?\\/u, "");
}

function mutationBlocked(path: string, directory: boolean): readonly boolean[] {
  let renameBlocked = false;
  try {
    const moved = `${path}.during-close`;
    renameSync(path, moved);
    renameSync(moved, path);
  } catch {
    renameBlocked = true;
  }
  let deleteBlocked = false;
  try {
    rmSync(path, { force: false, recursive: directory });
  } catch {
    deleteBlocked = true;
  }
  return [renameBlocked, deleteBlocked];
}

function expectReplacementAllowed(path: string, directory: boolean): void {
  const moved = `${path}.after-close`;
  expect(() => {
    if (!existsSync(path)) {
      if (directory) mkdirSync(path);
      else writeFileSync(path, "replacement");
    }
    renameSync(path, moved);
    renameSync(moved, path);
    rmSync(path, { force: false, recursive: directory });
    if (directory) mkdirSync(path);
    else writeFileSync(path, "replacement");
  }).not.toThrow();
}

function failAfterShmAnchor(delegate: PathSecurityVerifier): PathSecurityVerifier {
  let shmChecks = 0;
  return {
    verifyRequestedDataDirectory: (requested) => {
      delegate.verifyRequestedDataDirectory(requested);
    },
    verifyDataDirectory: (requested, canonical) =>
      delegate.verifyDataDirectory(requested, canonical),
    verifyDatabaseFile: (requested, canonical, artifact) => {
      const identity = delegate.verifyDatabaseFile(requested, canonical, artifact);
      if (artifact.endsWith("-shm") && existsSync(artifact)) {
        shmChecks += 1;
        if (shmChecks === 2) {
          throw new PathSecurityError("injected sidecar verification failure");
        }
      }
      return identity;
    },
    close: () => {
      delegate.close?.();
    },
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe.runIf(process.platform === "win32")("native store cleanup order", () => {
  it("closes file anchors before directories from leaf to root", async () => {
    const parent = temporaryDirectory("handles");
    const securityRoot = join(parent, "workspace-cache");
    const dataDirectory = join(securityRoot, "data");
    const store = openEventStore({
      dataDirectory,
      pathVerifier: createNativeEventStorePathVerifier({ securityRoot }),
    });
    await store.append("session-a", 0n, [
      { eventId: "event-a", type: "SessionOpened", payload: {} },
    ]);
    const closedPaths: string[] = [];
    const nativeClose = api.CloseHandle;
    vi.spyOn(api, "CloseHandle").mockImplementation((...arguments_: unknown[]) => {
      const handle = arguments_[0] as Handle;
      closedPaths.push(nativeHandlePath(handle));
      return nativeClose(...arguments_);
    });

    store.close();

    const firstDirectory = closedPaths.findIndex(
      (path) => !win32.basename(path).startsWith("lyonbrown.sqlite3"),
    );
    expect(firstDirectory).toBe(3);
    const directories = closedPaths.slice(firstDirectory);
    expect(win32.basename(directories[0] ?? "")).toBe("data");
    expect(win32.basename(directories[1] ?? "")).toBe("workspace-cache");
    for (let index = 1; index < directories.length; index += 1) {
      expect(win32.normalize(directories[index] ?? "").toLowerCase()).toBe(
        win32.dirname(directories[index - 1] ?? "").toLowerCase(),
      );
    }
  });

  it("keeps every anchor active throughout SQLite close", async () => {
    const parent = temporaryDirectory("normal");
    const securityRoot = join(parent, "workspace-cache");
    const dataDirectory = join(securityRoot, "data");
    const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
    const artifacts = [databasePath, `${databasePath}-wal`, `${databasePath}-shm`];
    const store = openEventStore({
      dataDirectory,
      pathVerifier: createNativeEventStorePathVerifier({ securityRoot }),
    });
    await store.append("session-a", 0n, [
      { eventId: "event-a", type: "SessionOpened", payload: {} },
    ]);
    const blocked: boolean[] = [];
    vi.spyOn(Database.prototype, "close").mockImplementation(function (this: Database.Database) {
      blocked.push(...mutationBlocked(dataDirectory, true));
      for (const artifact of artifacts) blocked.push(...mutationBlocked(artifact, false));
      return closeDatabase(this);
    });

    store.close();

    expect(blocked).toEqual(new Array(8).fill(true));
    for (const artifact of artifacts) expectReplacementAllowed(artifact, false);
    expectReplacementAllowed(dataDirectory, true);
  });

  it("keeps partial-open anchors until SQLite cleanup finishes", () => {
    const parent = temporaryDirectory("open-failure");
    const securityRoot = join(parent, "workspace-cache");
    const dataDirectory = join(securityRoot, "data");
    const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
    const artifacts = [databasePath, `${databasePath}-wal`, `${databasePath}-shm`];
    const blocked: boolean[] = [];
    vi.spyOn(Database.prototype, "close").mockImplementation(function (this: Database.Database) {
      blocked.push(...mutationBlocked(dataDirectory, true));
      for (const artifact of artifacts) blocked.push(...mutationBlocked(artifact, false));
      return closeDatabase(this);
    });
    const verifier = failAfterShmAnchor(createNativeEventStorePathVerifier({ securityRoot }));

    expect(() => openEventStore({ dataDirectory, pathVerifier: verifier })).toThrow(
      new PathSecurityError("injected sidecar verification failure"),
    );

    expect(blocked).toEqual(new Array(8).fill(true));
    for (const artifact of artifacts) expectReplacementAllowed(artifact, false);
    expectReplacementAllowed(dataDirectory, true);
  });
});
