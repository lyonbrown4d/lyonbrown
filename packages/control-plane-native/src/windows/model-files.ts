import { api } from "./api.ts";
import { ACCESS, ATTRIBUTES, CREATION, FLAGS, LOCK, SHARE } from "./constants.ts";
import { disposeOnClose } from "./files.ts";
import { createFile, failureForLastError, OwnedHandle, validateObjectType } from "./handles.ts";
import { openRegularFile } from "./handles.ts";
import { fileIdentity } from "./identity.ts";
import {
  currentUser,
  verifyPrivateInheritedFile,
  verifyPrivateObject,
  withPrivateSecurity,
} from "./security.ts";
import { NativeFailure } from "../result.ts";

function overlapped(): object {
  return { Internal: 0, InternalHigh: 0, Offset: 0, OffsetHigh: 0, hEvent: null };
}

export interface SharedFileLock {
  release(): void;
}

export function lockShared(handle: OwnedHandle): SharedFileLock {
  const state = overlapped();
  if (!api.LockFileEx(handle.borrow(), LOCK.failImmediately, 0, 0xffff_ffff, 0xffff_ffff, state)) {
    throw failureForLastError();
  }
  let released = false;
  return {
    release() {
      if (released) return;
      released = true;
      if (!api.UnlockFileEx(handle.borrow(), 0, 0xffff_ffff, 0xffff_ffff, state)) {
        throw failureForLastError();
      }
    },
  };
}

function verifyFile(handle: OwnedHandle): void {
  validateObjectType(handle, "file");
  const user = currentUser();
  try {
    verifyPrivateObject(handle.borrow(), user, false);
  } catch (error) {
    if (!(error instanceof NativeFailure) || error.failureCode !== "insecure_acl") throw error;
    verifyPrivateInheritedFile(handle.borrow(), user);
  }
}

export function createPrivateModelFile(path: string): OwnedHandle {
  const handle = withPrivateSecurity(currentUser(), false, (security) =>
    createFile(
      path,
      ACCESS.genericRead | ACCESS.genericWrite | ACCESS.delete | ACCESS.readControl,
      SHARE.read | SHARE.write,
      CREATION.createNew,
      ATTRIBUTES.normal | FLAGS.openReparsePoint | FLAGS.writeThrough,
      security,
    ),
  );
  try {
    verifyFile(handle);
    return handle;
  } catch (error) {
    handle.close();
    throw error;
  }
}

function createPrivateLockFile(path: string): OwnedHandle {
  const handle = withPrivateSecurity(currentUser(), false, (security) =>
    createFile(
      path,
      ACCESS.genericRead | ACCESS.genericWrite | ACCESS.readControl,
      SHARE.read | SHARE.write,
      CREATION.createNew,
      ATTRIBUTES.normal | FLAGS.openReparsePoint | FLAGS.writeThrough,
      security,
    ),
  );
  try {
    verifyFile(handle);
    return handle;
  } catch (error) {
    handle.close();
    throw error;
  }
}

export function openOrCreatePrivateLockFile(path: string): OwnedHandle {
  try {
    const existing = openRegularFile(
      path,
      ACCESS.genericRead | ACCESS.genericWrite,
      SHARE.read | SHARE.write,
    );
    try {
      verifyFile(existing);
      return existing;
    } catch (error) {
      existing.close();
      throw error;
    }
  } catch (error) {
    if (!(error instanceof NativeFailure) || error.failureCode !== "not_found") throw error;
  }
  try {
    return createPrivateLockFile(path);
  } catch {
    const raced = openRegularFile(
      path,
      ACCESS.genericRead | ACCESS.genericWrite,
      SHARE.read | SHARE.write,
    );
    try {
      verifyFile(raced);
      return raced;
    } catch (error) {
      raced.close();
      throw error;
    }
  }
}

export function openInstalledModel(path: string): OwnedHandle {
  const handle = createFile(
    path,
    ACCESS.genericRead | ACCESS.readControl,
    SHARE.read,
    CREATION.openExisting,
    FLAGS.openReparsePoint,
  );
  try {
    verifyFile(handle);
    return handle;
  } catch (error) {
    handle.close();
    throw error;
  }
}

export function openStagedModel(path: string): OwnedHandle {
  const handle = openRegularFile(
    path,
    ACCESS.genericRead | ACCESS.genericWrite | ACCESS.delete,
    SHARE.read | SHARE.write,
  );
  try {
    verifyFile(handle);
    return handle;
  } catch (error) {
    handle.close();
    throw error;
  }
}

export function deleteStagedModelIfPresent(path: string): void {
  let handle: OwnedHandle;
  try {
    handle = openRegularFile(
      path,
      ACCESS.genericRead | ACCESS.delete,
      SHARE.read | SHARE.write | SHARE.delete,
    );
  } catch (error) {
    if (error instanceof NativeFailure && error.failureCode === "not_found") return;
    throw error;
  }
  try {
    verifyFile(handle);
    disposeOnClose(handle);
  } finally {
    handle.close();
  }
}

export function reopenModelFile(source: OwnedHandle, shareMode: number): OwnedHandle {
  const handle = new OwnedHandle(
    api.ReOpenFile(
      source.borrow(),
      ACCESS.genericRead | ACCESS.readControl | ACCESS.fileReadAttributes,
      shareMode,
      FLAGS.openReparsePoint,
    ),
  );
  try {
    verifyFile(handle);
    return handle;
  } catch (error) {
    handle.close();
    throw error;
  }
}

export function flushModelFile(handle: OwnedHandle): void {
  if (!api.FlushFileBuffers(handle.borrow())) throw failureForLastError();
}

export function fileSize(handle: OwnedHandle, maximum: number): number {
  const output: unknown[] = [0n];
  if (!api.GetFileSizeEx(handle.borrow(), output)) throw failureForLastError();
  const value = BigInt(output[0] as bigint | number);
  if (value < 0 || value > BigInt(maximum) || value > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error("native_model_file_oversize");
  }
  return Number(value);
}

export function assertSameFile(handle: OwnedHandle, expectedIdentity: string): void {
  verifyFile(handle);
  if (fileIdentity(handle) !== expectedIdentity) throw new Error("native_model_identity_drift");
}
