import { win32 } from "node:path";

import { api } from "@lyonbrown/control-plane-native/internal/windows/api";
import {
  ACCESS,
  CREATION,
  ERRORS,
  FLAGS,
  SHARE,
} from "@lyonbrown/control-plane-native/internal/windows/constants";
import {
  createFile,
  failureForLastError,
  openDirectory,
  validateObjectType,
  type OwnedHandle,
} from "@lyonbrown/control-plane-native/internal/windows/handles";
import {
  canonicalPath,
  fileIdentity,
} from "@lyonbrown/control-plane-native/internal/windows/identity";
import {
  currentUser,
  verifyPrivateObject,
  withPrivateSecurity,
} from "@lyonbrown/control-plane-native/internal/windows/security";
import { NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

function wellFormed(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!Number.isInteger(next) || next < 0xdc00 || next > 0xdfff) return false;
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) return false;
  }
  return true;
}

function forbiddenSegmentUnit(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    if (value.charCodeAt(index) <= 0x1f) return true;
  }
  return /[<>:"|?*]/u.test(value);
}

function pathParts(path: string): { readonly root: string; readonly segments: readonly string[] } {
  if (!win32.isAbsolute(path) || path.includes("\0") || !wellFormed(path)) {
    throw new Error("native_model_directory_invalid");
  }
  const parsed = win32.parse(path);
  if (!/^[a-z]:[\\/]$/iu.test(parsed.root)) {
    throw new Error("native_model_directory_invalid");
  }
  const segments = path.slice(parsed.root.length).split(/[\\/]/u).filter(Boolean);
  if (
    segments.length < 2 ||
    segments.some(
      (segment) =>
        segment === "." ||
        segment === ".." ||
        forbiddenSegmentUnit(segment) ||
        segment.endsWith(".") ||
        segment.endsWith(" ") ||
        /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/iu.test(segment),
    )
  ) {
    throw new Error("native_model_directory_invalid");
  }
  return { root: parsed.root, segments };
}

function createPrivateDirectory(path: string): void {
  const user = currentUser();
  const created = withPrivateSecurity(user, true, (security) =>
    api.CreateDirectoryW(path, security),
  );
  if (!created && Number(api.GetLastError()) !== ERRORS.alreadyExists) {
    throw failureForLastError();
  }
}

function openOrCreate(path: string): OwnedHandle {
  try {
    return openDirectory(path, true);
  } catch (error) {
    if (!(error instanceof NativeFailure) || error.failureCode !== "not_found") throw error;
  }
  createPrivateDirectory(path);
  return openDirectory(path, true);
}

function closeAll(handles: readonly OwnedHandle[]): void {
  let failure: Error | undefined;
  for (const handle of handles.toReversed()) {
    try {
      handle.close();
    } catch (error) {
      failure ??= error instanceof Error ? error : new Error("native_model_storage_failed");
    }
  }
  if (failure !== undefined) throw failure;
}

export interface ModelDirectoryLease {
  readonly handle: OwnedHandle;
  readonly canonicalPath: string;
  readonly identity: string;
  close(): void;
}

export function validateModelsDirectory(path: string): string {
  pathParts(path);
  return win32.normalize(path);
}

export function openModelsDirectory(path: string): ModelDirectoryLease {
  const normalized = validateModelsDirectory(path);
  const { root, segments } = pathParts(normalized);
  const handles: OwnedHandle[] = [];
  try {
    let currentPath = root;
    let current = openDirectory(currentPath, true);
    handles.push(current);
    validateObjectType(current, "directory");
    for (const segment of segments) {
      currentPath = win32.join(currentPath, segment);
      current = openOrCreate(currentPath);
      handles.push(current);
      validateObjectType(current, "directory");
    }
    const parent = handles.at(-2);
    if (!parent) throw new Error("native_model_directory_invalid");
    const user = currentUser();
    verifyPrivateObject(parent.borrow(), user, true);
    verifyPrivateObject(current.borrow(), user, true);
    const canonical = canonicalPath(current);
    const identity = fileIdentity(current);
    let closed = false;
    return {
      handle: current,
      canonicalPath: canonical,
      identity,
      close() {
        if (closed) return;
        closed = true;
        closeAll(handles);
      },
    };
  } catch (error) {
    try {
      closeAll(handles);
    } catch {
      // Preserve the primary security failure.
    }
    throw error;
  }
}

export function createPrivateChildDirectory(
  parent: ModelDirectoryLease,
  path: string,
): OwnedHandle {
  createPrivateDirectory(path);
  const handle = createFile(
    path,
    ACCESS.genericRead | ACCESS.genericWrite | ACCESS.delete | ACCESS.readControl,
    SHARE.read | SHARE.write,
    CREATION.openExisting,
    FLAGS.backupSemantics | FLAGS.openReparsePoint | FLAGS.writeThrough,
  );
  try {
    parent.handle.borrow();
    validateObjectType(handle, "directory");
    verifyPrivateObject(handle.borrow(), currentUser(), true);
    return handle;
  } catch (error) {
    handle.close();
    throw error;
  }
}
