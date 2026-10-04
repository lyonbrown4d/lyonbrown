import { win32 } from "node:path";
import type {
  KernelFileLock,
  PlatformResult,
  SecureDirectoryHandle,
} from "@lyonbrown/local-control-plane/native-adapter";
import { api } from "./api.js";
import { ERRORS } from "./constants.js";
import { atomicReplace, readPrivateFile, tryLock } from "./files.js";
import { failureForLastError, openDirectory, validateObjectType } from "./handles.js";
import type { OwnedHandle } from "./handles.js";
import { canonicalPath } from "./identity.js";
import { currentUser, verifyPrivateObject, withPrivateSecurity } from "./security.js";
import { capture, NativeFailure } from "../result.js";

function closeAll(handles: readonly OwnedHandle[]): NativeFailure | undefined {
  let failure: NativeFailure | undefined;
  for (const handle of handles.toReversed()) {
    try {
      handle.close();
    } catch (error) {
      failure ??= error instanceof NativeFailure ? error : new NativeFailure("io");
    }
  }
  return failure;
}

class WindowsSecureDirectory implements SecureDirectoryHandle {
  readonly #handle: OwnedHandle;
  readonly #anchors: readonly OwnedHandle[];
  readonly #canonicalPath: string;
  #closeResult: Promise<PlatformResult<void>> | undefined;

  public constructor(handle: OwnedHandle, anchors: readonly OwnedHandle[], path: string) {
    this.#handle = handle;
    this.#anchors = anchors;
    this.#canonicalPath = path;
  }

  public readPrivateFile(name: "agentd.json", maximumBytes: number) {
    return capture(() => {
      this.#handle.borrow();
      return readPrivateFile(`${this.#canonicalPath}\\${name}`, maximumBytes);
    });
  }

  public atomicReplacePrivateFile(name: "agentd.json", bytes: Uint8Array) {
    return capture(() => {
      atomicReplace(this.#handle, this.#canonicalPath, name, bytes);
    });
  }

  public tryAcquireKernelLock(name: "agentd.start.lock" | "agentd.lock") {
    return capture<KernelFileLock | null>(() => {
      this.#handle.borrow();
      return tryLock(`${this.#canonicalPath}\\${name}`);
    });
  }

  public close(): Promise<PlatformResult<void>> {
    this.#closeResult ??= capture(() => {
      const failure = closeAll(this.#anchors);
      if (failure !== undefined) throw failure;
    });
    return this.#closeResult;
  }
}

function pathComponents(path: string): { root: string; segments: string[] } {
  if (!win32.isAbsolute(path) || path.includes("\0")) throw new NativeFailure("io");
  const parsed = win32.parse(path);
  const segments = path
    .slice(parsed.root.length)
    .split(/[\\/]/u)
    .filter((segment) => segment.length > 0);
  if (segments.some((segment) => segment === "." || segment === "..")) {
    throw new NativeFailure("io");
  }
  return { root: parsed.root, segments };
}

function openOrCreateComponent(
  path: string,
  user: ReturnType<typeof currentUser>,
): { handle: OwnedHandle; missing: boolean } {
  try {
    return { handle: openDirectory(path, true), missing: false };
  } catch (error) {
    if (!(error instanceof NativeFailure) || error.failureCode !== "not_found") throw error;
  }
  const created = withPrivateSecurity(user, true, (security) =>
    api.CreateDirectoryW(path, security),
  );
  if (!created && Number(api.GetLastError()) !== ERRORS.alreadyExists) {
    throw failureForLastError();
  }
  return { handle: openDirectory(path, true), missing: true };
}

export function openSecureDirectory(path: string): SecureDirectoryHandle {
  const { root, segments } = pathComponents(path);
  const user = currentUser();
  const traversed: OwnedHandle[] = [];
  try {
    let currentPath = root;
    let current = openDirectory(currentPath, true);
    traversed.push(current);
    validateObjectType(current, "directory");
    for (const segment of segments) {
      currentPath = win32.join(currentPath, segment);
      const next = openOrCreateComponent(currentPath, user);
      traversed.push(next.handle);
      validateObjectType(next.handle, "directory");
      if (next.missing) verifyPrivateObject(next.handle.borrow(), user, true);
      current = next.handle;
    }
    verifyPrivateObject(current.borrow(), user, true);
    const canonical = canonicalPath(current);
    return new WindowsSecureDirectory(current, traversed, canonical);
  } catch (error) {
    closeAll(traversed);
    throw error;
  }
}
