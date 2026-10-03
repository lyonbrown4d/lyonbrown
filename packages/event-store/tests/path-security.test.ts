import { existsSync, lstatSync, mkdtempSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { PathSecurityError, openEventStore, type PathSecurityVerifier } from "../src/index.js";
import { createPortablePathVerifier } from "../src/path-security.js";

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
  it("the portable verifier rejects junction ancestors before canonicalization", () => {
    const parent = temporaryDirectory("portable-parent");
    const target = temporaryDirectory("portable-target");
    const junction = join(parent, "junction");
    symlinkSync(target, junction, process.platform === "win32" ? "junction" : "dir");
    const verifier = createPortablePathVerifier();

    expect(() => {
      verifier.verifyRequestedDataDirectory(join(junction, "child"));
    }).toThrow(PathSecurityError);
    expect(verifier.verifyDataDirectory(target, target)).toBe(identity(target));
    expect(verifier.verifyDatabaseFile(target, target, join(target, "missing.sqlite3"))).toBe(
      undefined,
    );
    expect(() => verifier.verifyDatabaseFile(target, target, target)).toThrow(PathSecurityError);
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

  it("checks requested, canonical directory and database both before and after open", () => {
    const dataDirectory = temporaryDirectory("phases");
    const calls: string[] = [];
    const store = openEventStore({ dataDirectory, pathVerifier: allowingVerifier(calls) });
    store.close();

    expect(calls.filter((call) => call.startsWith("requested:"))).toHaveLength(2);
    expect(calls.filter((call) => call.startsWith("directory:"))).toHaveLength(2);
    expect(calls.filter((call) => call.startsWith("database:"))).toHaveLength(2);
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
});
