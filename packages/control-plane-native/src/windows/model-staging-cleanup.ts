import { readdirSync } from "node:fs";
import { win32 } from "node:path";

import { disposeOnClose } from "@lyonbrown/control-plane-native/internal/windows/files";
import type { OwnedHandle } from "@lyonbrown/control-plane-native/internal/windows/handles";
import {
  openPrivateChildDirectory,
  type ModelDirectoryLease,
} from "@lyonbrown/control-plane-native/internal/windows/model-directory";
import { deleteStagedModelIfPresent } from "@lyonbrown/control-plane-native/internal/windows/model-files";
import { closeQuietly } from "@lyonbrown/control-plane-native/internal/windows/model-storage-shared";

const MAX_STALE_STAGING_DIRECTORIES = 32;

export function cleanupStaleStagingDirectories(
  directory: ModelDirectoryLease,
  currentPath: string,
  prefix: string,
  modelFilename: string,
): void {
  const currentName = win32.basename(currentPath);
  const candidates = readdirSync(directory.canonicalPath, { withFileTypes: true })
    .filter((entry) => entry.name !== currentName && validStagingName(entry.name, prefix))
    .map((entry) => entry.name);
  if (candidates.length > MAX_STALE_STAGING_DIRECTORIES) {
    throw new Error("native_model_staging_limit");
  }
  for (const name of candidates) {
    const path = win32.join(directory.canonicalPath, name);
    let staging: OwnedHandle | undefined;
    try {
      staging = openPrivateChildDirectory(directory, path);
      deleteStagedModelIfPresent(win32.join(path, modelFilename));
      disposeOnClose(staging);
    } finally {
      closeQuietly(staging);
    }
  }
}

function validStagingName(name: string, prefix: string): boolean {
  if (!name.startsWith(prefix)) return false;
  const suffix = name.slice(prefix.length);
  if (suffix.length !== 32) return false;
  for (const character of suffix) {
    const code = character.codePointAt(0);
    if (code === undefined || !((code >= 48 && code <= 57) || (code >= 97 && code <= 102))) {
      return false;
    }
  }
  return true;
}
