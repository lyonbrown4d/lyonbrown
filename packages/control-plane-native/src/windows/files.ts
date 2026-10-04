import { randomBytes } from "node:crypto";
import type { KernelFileLock, PlatformResult } from "@lyonbrown/local-control-plane/native-adapter";
import { api, pointerAddress, pointerSize, type Handle } from "./api.js";
import {
  ACCESS,
  ATTRIBUTES,
  CREATION,
  ERRORS,
  FILE_INFO,
  FLAGS,
  LOCK,
  SHARE,
} from "./constants.js";
import {
  createFile,
  failureForLastError,
  failureForNtStatus,
  openRegularFile,
  validateObjectType,
} from "./handles.js";
import type { OwnedHandle } from "./handles.js";
import { currentUser, verifyPrivateObject, withPrivateSecurity } from "./security.js";
import { capture, NativeFailure } from "../result.js";

function childPath(canonicalDirectory: string, name: string): string {
  return `${canonicalDirectory}\\${name}`;
}

function secureExistingFile(path: string): OwnedHandle | null {
  let handle: OwnedHandle;
  try {
    handle = openRegularFile(path, ACCESS.genericRead, SHARE.read);
  } catch (error) {
    if ((error as NativeFailure).failureCode === "not_found") return null;
    throw error;
  }
  try {
    validateObjectType(handle, "file");
    verifyPrivateObject(handle.borrow(), currentUser(), false);
    return handle;
  } catch (error) {
    handle.close();
    throw error;
  }
}

export function readPrivateFile(path: string, maximumBytes: number): Uint8Array | null {
  if (!Number.isSafeInteger(maximumBytes) || maximumBytes < 0) throw failureForLastError(87);
  const handle = secureExistingFile(path);
  if (!handle) return null;
  try {
    const size: unknown[] = [0n];
    if (!api.GetFileSizeEx(handle.borrow(), size)) throw failureForLastError();
    const length = BigInt(size[0] as bigint | number);
    if (length > BigInt(maximumBytes)) {
      throw new NativeFailure("oversize");
    }
    const bytes = Buffer.alloc(Number(length));
    const read: unknown[] = [0];
    if (bytes.length > 0 && !api.ReadFile(handle.borrow(), bytes, bytes.length, read, null)) {
      throw failureForLastError();
    }
    if (Number(read[0]) !== bytes.length) throw failureForLastError(38);
    return bytes;
  } finally {
    handle.close();
  }
}

function renamePayload(directory: Handle, destination: string): Buffer {
  const name = Buffer.from(destination, "utf16le");
  const rootOffset = pointerSize === 8 ? 8 : 4;
  const lengthOffset = rootOffset + pointerSize;
  const nameOffset = lengthOffset + 4;
  const structureSize = pointerSize === 8 ? 24 : 16;
  const payload = Buffer.alloc(structureSize + name.length);
  payload.writeUInt8(1, 0);
  if (pointerSize === 8) payload.writeBigUInt64LE(pointerAddress(directory), rootOffset);
  else payload.writeUInt32LE(Number(pointerAddress(directory) & 0xffff_ffffn), rootOffset);
  payload.writeUInt32LE(name.length, lengthOffset);
  name.copy(payload, nameOffset);
  return payload;
}

function disposeOnClose(handle: OwnedHandle): void {
  const disposition = Buffer.from([1]);
  if (
    !api.SetFileInformationByHandle(
      handle.borrow(),
      FILE_INFO.disposition,
      disposition,
      disposition.length,
    )
  ) {
    throw failureForLastError();
  }
}

export function atomicReplace(
  directory: OwnedHandle,
  canonicalDirectory: string,
  destination: string,
  bytes: Uint8Array,
): void {
  const existing = secureExistingFile(childPath(canonicalDirectory, destination));
  existing?.close();
  const temporary = `.agentd-${randomBytes(16).toString("hex")}.tmp`;
  const user = currentUser();
  const handle = withPrivateSecurity(user, false, (security) =>
    createFile(
      childPath(canonicalDirectory, temporary),
      ACCESS.genericRead | ACCESS.genericWrite | ACCESS.delete | ACCESS.readControl,
      0,
      CREATION.createNew,
      ATTRIBUTES.normal | FLAGS.openReparsePoint | FLAGS.writeThrough,
      security,
    ),
  );
  let renamed = false;
  try {
    validateObjectType(handle, "file");
    verifyPrivateObject(handle.borrow(), user, false);
    const written: unknown[] = [0];
    const input = Buffer.from(bytes);
    if (input.length > 0 && !api.WriteFile(handle.borrow(), input, input.length, written, null)) {
      throw failureForLastError();
    }
    if (Number(written[0]) !== input.length) throw failureForLastError(29);
    if (!api.FlushFileBuffers(handle.borrow())) throw failureForLastError();
    const payload = renamePayload(directory.borrow(), destination);
    const ioStatus = Buffer.alloc(pointerSize * 2);
    const status = Number(
      api.NtSetInformationFile(handle.borrow(), ioStatus, payload, payload.length, 10),
    );
    if (status < 0) throw failureForNtStatus(status);
    renamed = true;
  } finally {
    try {
      if (!renamed) disposeOnClose(handle);
    } finally {
      handle.close();
    }
  }
}

function emptyOverlapped(): object {
  return { Internal: 0, InternalHigh: 0, Offset: 0, OffsetHigh: 0, hEvent: null };
}

class WindowsKernelLock implements KernelFileLock {
  readonly #handle: OwnedHandle;
  readonly #overlapped: object;
  #release: Promise<PlatformResult<void>> | undefined;

  public constructor(handle: OwnedHandle, overlapped: object) {
    this.#handle = handle;
    this.#overlapped = overlapped;
  }

  public release() {
    this.#release ??= capture(() => {
      try {
        if (
          !api.UnlockFileEx(this.#handle.borrow(), 0, 0xffff_ffff, 0xffff_ffff, this.#overlapped)
        ) {
          throw failureForLastError();
        }
      } finally {
        this.#handle.close();
      }
    });
    return this.#release;
  }
}

export function tryLock(path: string): KernelFileLock | null {
  const user = currentUser();
  const handle = withPrivateSecurity(user, false, (security) =>
    createFile(
      path,
      ACCESS.genericRead | ACCESS.genericWrite | ACCESS.readControl,
      SHARE.read | SHARE.write,
      CREATION.openAlways,
      ATTRIBUTES.normal | FLAGS.openReparsePoint,
      security,
    ),
  );
  try {
    validateObjectType(handle, "file");
    verifyPrivateObject(handle.borrow(), user, false);
    const overlapped = emptyOverlapped();
    if (
      !api.LockFileEx(
        handle.borrow(),
        LOCK.exclusive | LOCK.failImmediately,
        0,
        0xffff_ffff,
        0xffff_ffff,
        overlapped,
      )
    ) {
      if (Number(api.GetLastError()) === ERRORS.lockViolation) {
        handle.close();
        return null;
      }
      throw failureForLastError();
    }
    return new WindowsKernelLock(handle, overlapped);
  } catch (error) {
    handle.close();
    throw error;
  }
}
