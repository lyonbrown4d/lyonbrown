import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdtempSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import {
  PathSecurityError,
  openEventStore,
  type PathSecurityVerifier,
} from "@lyonbrown/event-store/internal/index";
import {
  assertSecurePosixDirectoryMetadata,
  assertSecurePosixFileMetadata,
  createPortablePathVerifier,
} from "@lyonbrown/event-store/internal/path-security";

const directories: string[] = [];

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

function allowingVerifier(calls: string[]): PathSecurityVerifier {
  return {
    verifyRequestedDataDirectory: (requestedPath) => {
      calls.push(`requested:${requestedPath}`);
    },
    verifyDataDirectory: (requestedPath, canonicalPath) => {
      calls.push(`directory:${requestedPath}:${canonicalPath}`);
      return identity(canonicalPath) ?? "missing-directory";
    },
    verifyDatabaseFile: (requestedPath, canonicalPath, databasePath) => {
      calls.push(`database:${requestedPath}:${canonicalPath}:${databasePath}`);
      return identity(databasePath);
    },
  };
}

afterEach(() => {
  for (const directory of directories.splice(0))
    rmSync(directory, { force: true, recursive: true });
});

describe("path security phases", () => {
  it.runIf(process.platform !== "win32")(
    "fails closed for an existing permissive POSIX directory without chmodding it",
    () => {
      const dataDirectory = temporaryDirectory("permissive-directory");
      chmodSync(dataDirectory, 0o755);

      expect(() => openEventStore({ dataDirectory })).toThrow(PathSecurityError);
      expect(statSync(dataDirectory).mode & 0o777).toBe(0o755);
    },
  );

  it.runIf(process.platform !== "win32")(
    "creates 0700/0600 storage and securely reopens it",
    () => {
      const parent = temporaryDirectory("secure-create");
      const dataDirectory = join(parent, "data");
      const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
      const store = openEventStore({ dataDirectory });

      expect(statSync(dataDirectory).mode & 0o777).toBe(0o700);
      expect(statSync(databasePath).mode & 0o777).toBe(0o600);
      for (const suffix of ["-wal", "-shm"] as const) {
        const sidecar = `${databasePath}${suffix}`;
        if (existsSync(sidecar)) expect(statSync(sidecar).mode & 0o077).toBe(0);
      }
      store.close();

      const reopened = openEventStore({ dataDirectory });
      reopened.close();
    },
  );

  it.runIf(process.platform !== "win32")(
    "rejects an insecure existing database or WAL sidecar",
    () => {
      const dataDirectory = temporaryDirectory("insecure-artifact");
      const databasePath = join(dataDirectory, "lyonbrown.sqlite3");
      const first = openEventStore({ dataDirectory });
      first.close();
      chmodSync(databasePath, 0o644);

      expect(() => openEventStore({ dataDirectory })).toThrow(PathSecurityError);
      chmodSync(databasePath, 0o600);
      writeFileSync(`${databasePath}-wal`, "not a real WAL", { mode: 0o644 });
      expect(() => openEventStore({ dataDirectory })).toThrow(PathSecurityError);
    },
  );

  it.each([0o755, 0o777])("rejects a POSIX data directory with mode %s", (mode) => {
    expect(() => {
      assertSecurePosixDirectoryMetadata({ kind: "directory", mode, uid: 1000 }, 1000);
    }).toThrow(PathSecurityError);
  });

  it("requires POSIX storage to belong to the daemon uid", () => {
    expect(() => {
      assertSecurePosixDirectoryMetadata({ kind: "directory", mode: 0o700, uid: 1001 }, 1000);
    }).toThrow(PathSecurityError);
    expect(() => {
      assertSecurePosixFileMetadata({ kind: "file", mode: 0o600, uid: 1001 }, 1000, "database");
    }).toThrow(PathSecurityError);
  });

  it.each([0o700, 0o640, 0o606])("rejects a POSIX database artifact with mode %s", (mode) => {
    expect(() => {
      assertSecurePosixFileMetadata({ kind: "file", mode, uid: 1000 }, 1000, "database");
    }).toThrow(PathSecurityError);
  });

  it("accepts a private POSIX directory and regular 0600 database artifact", () => {
    expect(() => {
      assertSecurePosixDirectoryMetadata({ kind: "directory", mode: 0o700, uid: 1000 }, 1000);
    }).not.toThrow();
    expect(() => {
      assertSecurePosixFileMetadata({ kind: "file", mode: 0o600, uid: 1000 }, 1000, "database");
    }).not.toThrow();
  });

  it("rejects non-directory and non-file POSIX metadata", () => {
    expect(() => {
      assertSecurePosixDirectoryMetadata({ kind: "file", mode: 0o700, uid: 1000 }, 1000);
    }).toThrow(PathSecurityError);
    expect(() => {
      assertSecurePosixFileMetadata({ kind: "other", mode: 0o600, uid: 1000 }, 1000, "WAL");
    }).toThrow(PathSecurityError);
  });

  it("the portable verifier rejects junction ancestors before canonicalization", () => {
    const parent = temporaryDirectory("portable-parent");
    const target = temporaryDirectory("portable-target");
    const junction = join(parent, "junction");
    symlinkSync(target, junction, process.platform === "win32" ? "junction" : "dir");
    const verifier = createPortablePathVerifier();

    expect(() => {
      verifier.verifyRequestedDataDirectory(join(junction, "child"));
    }).toThrow(PathSecurityError);
    expect(() => verifier.verifyDataDirectory(junction, junction)).toThrow(PathSecurityError);
    expect(() => verifier.verifyDatabaseFile(target, target, junction)).toThrow(PathSecurityError);
    expect(verifier.verifyDatabaseFile(target, target, join(target, "missing.sqlite3"))).toBe(
      undefined,
    );
    if (process.platform !== "win32") {
      expect(verifier.verifyDataDirectory(target, target)).toBe(identity(target));
      expect(() => verifier.verifyDatabaseFile(target, target, target)).toThrow(PathSecurityError);
    } else {
      expect(() => verifier.verifyDataDirectory(target, target)).toThrow(PathSecurityError);
    }
  });

  it("presents a junction requested path before realpath can wash it into its target", () => {
    const parent = temporaryDirectory("junction-parent");
    const target = temporaryDirectory("junction-target");
    const junction = join(parent, "data-link");
    symlinkSync(target, junction, process.platform === "win32" ? "junction" : "dir");
    const seen: string[] = [];
    const verifier = allowingVerifier(seen);
    verifier.verifyRequestedDataDirectory = (requestedPath) => {
      seen.push(`requested:${requestedPath}`);
      if (lstatSync(requestedPath).isSymbolicLink()) {
        throw new PathSecurityError("requested path contains a junction");
      }
    };

    expect(() => openEventStore({ dataDirectory: junction, pathVerifier: verifier })).toThrow(
      PathSecurityError,
    );
    expect(seen[0]).toBe(`requested:${junction}`);
  });

  it("checks the directory, database and WAL sidecars before and after open", () => {
    const dataDirectory = temporaryDirectory("phases");
    const calls: string[] = [];
    const store = openEventStore({ dataDirectory, pathVerifier: allowingVerifier(calls) });
    store.close();

    expect(calls.filter((call) => call.startsWith("requested:"))).toHaveLength(2);
    expect(calls.filter((call) => call.startsWith("directory:"))).toHaveLength(2);
    expect(calls.filter((call) => call.endsWith("lyonbrown.sqlite3"))).toHaveLength(2);
    expect(calls.filter((call) => call.endsWith("lyonbrown.sqlite3-wal"))).toHaveLength(2);
    expect(calls.filter((call) => call.endsWith("lyonbrown.sqlite3-shm"))).toHaveLength(2);
  });

  it.each(["directory", "database"] as const)(
    "fails closed when %s identity changes across open",
    (kind) => {
      const dataDirectory = temporaryDirectory(`identity-${kind}`);
      const calls: string[] = [];
      const base = allowingVerifier(calls);
      let checks = 0;
      const verifier: PathSecurityVerifier = {
        ...base,
        ...(kind === "directory"
          ? {
              verifyDataDirectory: () => {
                checks += 1;
                return `directory-${String(checks)}`;
              },
            }
          : {
              verifyDatabaseFile: () => {
                checks += 1;
                return `database-${String(checks)}`;
              },
            }),
      };

      expect(() => openEventStore({ dataDirectory, pathVerifier: verifier })).toThrow(
        PathSecurityError,
      );
    },
  );

  it("fails closed when the database disappears during open", () => {
    const dataDirectory = temporaryDirectory("identity-missing-database");
    const base = allowingVerifier([]);
    const verifier: PathSecurityVerifier = {
      ...base,
      verifyDatabaseFile: (_requested, _canonical, databasePath) => {
        if (!databasePath.endsWith("lyonbrown.sqlite3")) return identity(databasePath);
        return undefined;
      },
    };

    expect(() => openEventStore({ dataDirectory, pathVerifier: verifier })).toThrow(
      PathSecurityError,
    );
  });
});
