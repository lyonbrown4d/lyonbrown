import { existsSync, mkdtempSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { openEventStore, PathSecurityError } from "@lyonbrown/event-store";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createNativeEventStorePathVerifier } from "../src/index.js";
import { api } from "../src/windows/api.js";

const temporaryDirectories: string[] = [];

function temporaryDirectory(label: string): string {
  const path = mkdtempSync(join(tmpdir(), `lyonbrown-native-store-${label}-`));
  temporaryDirectories.push(path);
  return path;
}

afterEach(() => {
  vi.restoreAllMocks();
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

describe.runIf(process.platform === "win32")("native Windows event-store verifier", () => {
  it("creates private storage, supports real WAL I/O, and anchors paths until close", async () => {
    const parent = temporaryDirectory("wal");
    const securityRoot = join(parent, "workspace-cache");
    const dataDirectory = join(securityRoot, "data");
    const moved = join(parent, "moved-cache");
    const store = openEventStore({
      dataDirectory,
      pathVerifier: createNativeEventStorePathVerifier({ securityRoot }),
    });

    await store.append("session-a", 0n, [
      { eventId: "event-a", type: "SessionOpened", payload: { cwd: "C:\\work" } },
    ]);
    expect(store.diagnostics().journalMode.toLowerCase()).toBe("wal");
    expect(existsSync(join(dataDirectory, "lyonbrown.sqlite3-wal"))).toBe(true);
    expect(() => {
      renameSync(securityRoot, moved);
    }).toThrow();

    store.close();
    expect(() => {
      renameSync(securityRoot, moved);
    }).not.toThrow();
  });

  it("anchors requested directories across the verify-to-open boundary", () => {
    const parent = temporaryDirectory("directory-race");
    const securityRoot = join(parent, "workspace-cache");
    const dataDirectory = join(securityRoot, "data");
    const verifier = createNativeEventStorePathVerifier({ securityRoot });

    verifier.verifyRequestedDataDirectory(dataDirectory);
    expect(() => {
      renameSync(dataDirectory, join(securityRoot, "replaced"));
    }).toThrow();
    expect(() => {
      rmSync(dataDirectory, { recursive: true });
    }).toThrow();
    verifier.close?.();
    expect(() => {
      renameSync(dataDirectory, join(securityRoot, "replaced"));
    }).not.toThrow();
  });

  it("rejects a final-directory junction below a private security root", () => {
    const parent = temporaryDirectory("final-junction");
    const target = temporaryDirectory("final-junction-target");
    const securityRoot = join(parent, "workspace-cache");
    const seed = join(securityRoot, "seed");
    const bootstrap = createNativeEventStorePathVerifier({ securityRoot });
    bootstrap.verifyRequestedDataDirectory(seed);
    bootstrap.close?.();
    rmSync(seed, { recursive: true });
    const dataDirectory = join(securityRoot, "data");
    symlinkSync(target, dataDirectory, "junction");

    const verifier = createNativeEventStorePathVerifier({ securityRoot });
    expect(() => {
      verifier.verifyRequestedDataDirectory(dataDirectory);
    }).toThrow(PathSecurityError);
    verifier.close?.();
  });

  it("rejects a junction at the security root or below it", () => {
    const parent = temporaryDirectory("junction");
    const target = temporaryDirectory("junction-target");
    const securityRoot = join(parent, "workspace-cache");
    symlinkSync(target, securityRoot, "junction");

    const verifier = createNativeEventStorePathVerifier({ securityRoot });
    expect(() => {
      verifier.verifyRequestedDataDirectory(join(securityRoot, "data"));
    }).toThrow(PathSecurityError);
    verifier.close?.();
  });

  it("rejects an existing security root with a broad inherited ACL", () => {
    const securityRoot = temporaryDirectory("broad-acl");
    const verifier = createNativeEventStorePathVerifier({ securityRoot });

    expect(() => {
      verifier.verifyRequestedDataDirectory(join(securityRoot, "data"));
    }).toThrow(PathSecurityError);
    verifier.close?.();
  });

  it("anchors existing database, WAL and SHM files against replacement", async () => {
    const parent = temporaryDirectory("artifacts");
    const securityRoot = join(parent, "workspace-cache");
    const dataDirectory = join(securityRoot, "data");
    const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
    const store = openEventStore({
      dataDirectory,
      pathVerifier: createNativeEventStorePathVerifier({ securityRoot }),
    });
    await store.append("session-a", 0n, [
      { eventId: "event-a", type: "SessionOpened", payload: {} },
    ]);

    for (const path of [databasePath, `${databasePath}-wal`, `${databasePath}-shm`]) {
      expect(existsSync(path)).toBe(true);
      expect(() => {
        renameSync(path, `${path}.replaced`);
      }).toThrow();
    }
    store.close();
    expect(() => {
      renameSync(databasePath, `${databasePath}.closed`);
    }).not.toThrow();
  });

  it.each(["", "-wal", "-shm"])("rejects a reparse database artifact %j", (suffix) => {
    const parent = temporaryDirectory(`artifact-link${suffix || "-db"}`);
    const securityRoot = join(parent, "workspace-cache");
    const dataDirectory = join(securityRoot, "data");
    const bootstrap = createNativeEventStorePathVerifier({ securityRoot });
    bootstrap.verifyRequestedDataDirectory(dataDirectory);
    bootstrap.close?.();
    const target = join(parent, `target${suffix || "-db"}`);
    writeFileSync(target, "not sqlite");
    symlinkSync(target, join(dataDirectory, `lyonbrown.sqlite3${suffix}`), "file");

    expect(() =>
      openEventStore({
        dataDirectory,
        pathVerifier: createNativeEventStorePathVerifier({ securityRoot }),
      }),
    ).toThrow(PathSecurityError);
  });

  it("rejects invalid roots, scope changes, and paths outside the owned boundary", () => {
    for (const path of ["relative", "C:\\bad\\..\\root", "C:\\bad\0root", "C:\\bad\ud800root"]) {
      expect(() => createNativeEventStorePathVerifier({ securityRoot: path })).toThrow(
        PathSecurityError,
      );
    }
    const parent = temporaryDirectory("boundaries");
    const securityRoot = join(parent, "workspace-cache");
    const dataDirectory = join(securityRoot, "data");
    const verifier = createNativeEventStorePathVerifier({ securityRoot });
    verifier.verifyRequestedDataDirectory(dataDirectory);
    expect(() => {
      verifier.verifyDataDirectory(dataDirectory, `${dataDirectory}\\.`);
    }).toThrow(PathSecurityError);
    expect(() => {
      verifier.verifyRequestedDataDirectory(join(securityRoot, "other"));
    }).toThrow(PathSecurityError);
    expect(() =>
      verifier.verifyDatabaseFile(dataDirectory, dataDirectory, join(parent, "outside.sqlite3")),
    ).toThrow(PathSecurityError);
    verifier.close?.();
    verifier.close?.();
    expect(() => {
      verifier.verifyRequestedDataDirectory(dataDirectory);
    }).toThrow(PathSecurityError);
  });

  it("supports an inferred security root and stable repeated identities", () => {
    const parent = temporaryDirectory("inferred-root");
    const dataDirectory = join(parent, "data");
    const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
    const verifier = createNativeEventStorePathVerifier();
    verifier.verifyRequestedDataDirectory(dataDirectory);
    const firstDirectory = verifier.verifyDataDirectory(dataDirectory, dataDirectory);
    const secondDirectory = verifier.verifyDataDirectory(dataDirectory, dataDirectory);
    const firstDatabase = verifier.verifyDatabaseFile(dataDirectory, dataDirectory, databasePath);
    const secondDatabase = verifier.verifyDatabaseFile(dataDirectory, dataDirectory, databasePath);
    expect(firstDirectory).toBe(secondDirectory);
    expect(firstDatabase).toBe(secondDatabase);
    expect(
      verifier.verifyDatabaseFile(dataDirectory, dataDirectory, `${databasePath}-wal`),
    ).toBeUndefined();
    verifier.close?.();
  });

  it("closes every anchor and redacts a native close failure", () => {
    const parent = temporaryDirectory("close-failure");
    const dataDirectory = join(parent, "data");
    const verifier = createNativeEventStorePathVerifier();
    verifier.verifyRequestedDataDirectory(dataDirectory);
    const nativeClose = api.CloseHandle;
    let injected = false;
    vi.spyOn(api, "CloseHandle").mockImplementation((handle: unknown) => {
      const result = Number(nativeClose(handle));
      if (!injected) {
        injected = true;
        return 0;
      }
      return result;
    });

    expect(() => verifier.close?.()).toThrow(
      new PathSecurityError("Native Windows storage path verification failed"),
    );
    expect(() => verifier.close?.()).not.toThrow();
  });
});
