import { existsSync, lstatSync, mkdirSync, realpathSync } from "node:fs";
import { isAbsolute, join, normalize, parse, resolve, sep } from "node:path";

import { InvalidStoreOptionsError, PathSecurityError } from "./errors.js";
import type { PathSecurityVerifier } from "./types.js";

const DATABASE_FILENAME = "lyonbrown.sqlite3";

function identity(path: string): string {
  const stats = lstatSync(path, { bigint: true });
  return `${stats.dev.toString()}:${stats.ino.toString()}`;
}

function inspectRequestedAncestors(requestedPath: string): void {
  const { root } = parse(requestedPath);
  let current = root;
  for (const component of requestedPath
    .slice(root.length)
    .split(sep)
    .filter((part) => part.length > 0)) {
    current = join(current, component);
    if (!existsSync(current)) break;
    if (lstatSync(current).isSymbolicLink()) {
      throw new PathSecurityError(`Requested data path traverses a symlink: ${current}`);
    }
  }
}

const portableVerifier: PathSecurityVerifier = {
  verifyRequestedDataDirectory: (requestedPath) => {
    inspectRequestedAncestors(requestedPath);
  },
  verifyDataDirectory: (_requestedPath, canonicalPath) => {
    const stats = lstatSync(canonicalPath);
    if (!stats.isDirectory() || stats.isSymbolicLink()) {
      throw new PathSecurityError("Canonical data directory must be a real directory");
    }
    return identity(canonicalPath);
  },
  verifyDatabaseFile: (_requestedPath, _canonicalPath, databasePath) => {
    if (!existsSync(databasePath)) return undefined;
    const stats = lstatSync(databasePath);
    if (!stats.isFile() || stats.isSymbolicLink()) {
      throw new PathSecurityError("Database must be a regular non-symlink file");
    }
    return identity(databasePath);
  },
};

export function createPortablePathVerifier(): PathSecurityVerifier {
  return portableVerifier;
}

export interface PreparedDatabasePath {
  readonly requestedDataDirectory: string;
  readonly dataDirectory: string;
  readonly databasePath: string;
  readonly directoryIdentity: string;
  readonly databaseIdentity: string | undefined;
}

export function defaultPathVerifier(): PathSecurityVerifier {
  if (process.platform === "win32") {
    throw new PathSecurityError(
      "Windows production storage requires an injected verifier for reparse points and DACLs",
    );
  }
  return createPortablePathVerifier();
}

export function prepareDatabasePath(
  dataDirectory: string,
  verifier: PathSecurityVerifier,
): PreparedDatabasePath {
  if (!isAbsolute(dataDirectory)) {
    throw new InvalidStoreOptionsError("dataDirectory must be an absolute path");
  }
  const requested = normalize(resolve(dataDirectory));
  verifier.verifyRequestedDataDirectory(requested);
  mkdirSync(requested, { recursive: true, mode: 0o700 });
  const canonical = realpathSync.native(requested);
  const directoryIdentity = verifier.verifyDataDirectory(requested, canonical);

  const databasePath = join(canonical, DATABASE_FILENAME);
  const databaseIdentity = verifier.verifyDatabaseFile(requested, canonical, databasePath);
  return {
    requestedDataDirectory: requested,
    dataDirectory: canonical,
    databasePath,
    directoryIdentity,
    databaseIdentity,
  };
}

export function reverifyDatabasePath(
  prepared: PreparedDatabasePath,
  verifier: PathSecurityVerifier,
): void {
  verifier.verifyRequestedDataDirectory(prepared.requestedDataDirectory);
  const directoryIdentity = verifier.verifyDataDirectory(
    prepared.requestedDataDirectory,
    prepared.dataDirectory,
  );
  const databaseIdentity = verifier.verifyDatabaseFile(
    prepared.requestedDataDirectory,
    prepared.dataDirectory,
    prepared.databasePath,
  );
  if (directoryIdentity !== prepared.directoryIdentity) {
    throw new PathSecurityError("Data directory identity changed while opening SQLite");
  }
  if (databaseIdentity === undefined) {
    throw new PathSecurityError("SQLite database is missing after open");
  }
  if (prepared.databaseIdentity !== undefined && databaseIdentity !== prepared.databaseIdentity) {
    throw new PathSecurityError("SQLite database identity changed while opening");
  }
}
