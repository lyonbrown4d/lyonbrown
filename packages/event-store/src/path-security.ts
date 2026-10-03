import { lstatSync, mkdirSync, realpathSync } from "node:fs";
import { isAbsolute, join, normalize, resolve } from "node:path";

import { InvalidStoreOptionsError, PathSecurityError } from "./errors.js";
import type { PathSecurityVerifier } from "./types.js";

const DATABASE_FILENAME = "lyonbrown.sqlite3";

function rejectSymbolicLink(path: string, role: string): void {
  try {
    if (lstatSync(path).isSymbolicLink())
      throw new PathSecurityError(`${role} must not be a symlink`);
  } catch (error: unknown) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code !== "ENOENT") throw error;
  }
}

const portableVerifier: PathSecurityVerifier = {
  verifyDataDirectory: (path) => {
    rejectSymbolicLink(path, "Data directory");
  },
  verifyDatabaseFile: (path) => {
    rejectSymbolicLink(path, "Database file");
  },
};

export function defaultPathVerifier(): PathSecurityVerifier {
  if (process.platform === "win32") {
    throw new PathSecurityError(
      "Windows production storage requires an injected verifier for reparse points and DACLs",
    );
  }
  return portableVerifier;
}

export function prepareDatabasePath(
  dataDirectory: string,
  verifier: PathSecurityVerifier,
): { readonly dataDirectory: string; readonly databasePath: string } {
  if (!isAbsolute(dataDirectory)) {
    throw new InvalidStoreOptionsError("dataDirectory must be an absolute path");
  }
  const requested = normalize(resolve(dataDirectory));
  mkdirSync(requested, { recursive: true, mode: 0o700 });
  const canonical = realpathSync.native(requested);
  verifier.verifyDataDirectory(canonical);

  const databasePath = join(canonical, DATABASE_FILENAME);
  verifier.verifyDatabaseFile(databasePath);
  return { dataDirectory: canonical, databasePath };
}
