import { api, pointerAddress, type Handle } from "./api.js";
import { ACCESS, ATTRIBUTES, CREATION, ERRORS, FLAGS, SHARE } from "./constants.js";
import { NativeFailure } from "../result.js";

const INVALID_HANDLE = (1n << BigInt(process.arch === "ia32" ? 32 : 64)) - 1n;

function lastError(): number {
  return Number(api.GetLastError());
}

export function failureForLastError(error = lastError()): NativeFailure {
  switch (error) {
    case ERRORS.fileNotFound:
    case ERRORS.pathNotFound:
      return new NativeFailure("not_found");
    case ERRORS.accessDenied:
    case ERRORS.sharingViolation:
      return new NativeFailure("access_denied");
    default:
      return new NativeFailure("io");
  }
}

export function failureForNtStatus(status: number): NativeFailure {
  switch (status >>> 0) {
    case 0xc000_0022:
      return new NativeFailure("access_denied");
    case 0xc000_0034:
    case 0xc000_003a:
      return new NativeFailure("not_found");
    case 0xc000_050b:
      return new NativeFailure("reparse_point");
    default:
      return new NativeFailure("io");
  }
}

export class OwnedHandle {
  readonly #value: Handle;
  #closed = false;

  public constructor(value: unknown) {
    if ((typeof value !== "object" && typeof value !== "bigint") || value === null) {
      throw failureForLastError();
    }
    const handle = value as Handle;
    if (pointerAddress(handle) === INVALID_HANDLE) throw failureForLastError();
    this.#value = handle;
  }

  public borrow(): Handle {
    if (this.#closed) throw new NativeFailure("io");
    return this.#value;
  }

  public close(): void {
    if (this.#closed) return;
    this.#closed = true;
    if (!api.CloseHandle(this.#value)) throw new NativeFailure("io");
  }
}

export function createFile(
  path: string,
  desiredAccess: number,
  shareMode: number,
  creation: number,
  flags: number,
  securityAttributes: object | null = null,
): OwnedHandle {
  return new OwnedHandle(
    api.CreateFileW(path, desiredAccess, shareMode, securityAttributes, creation, flags, null),
  );
}

export function openDirectory(path: string, anchor: boolean): OwnedHandle {
  return createFile(
    path,
    ACCESS.genericRead |
      ACCESS.readControl |
      ACCESS.fileListDirectory |
      ACCESS.fileTraverse |
      ACCESS.fileReadAttributes,
    SHARE.read | SHARE.write | (anchor ? 0 : SHARE.delete),
    CREATION.openExisting,
    FLAGS.backupSemantics | FLAGS.openReparsePoint,
  );
}

export function openWorkspaceDirectory(path: string): OwnedHandle {
  return createFile(
    path,
    0,
    SHARE.read | SHARE.write | SHARE.delete,
    CREATION.openExisting,
    FLAGS.backupSemantics,
  );
}

export function openRegularFile(path: string, access: number, share: number): OwnedHandle {
  return createFile(
    path,
    access | ACCESS.readControl | ACCESS.fileReadAttributes,
    share,
    CREATION.openExisting,
    FLAGS.openReparsePoint,
  );
}

export function validateObjectType(handle: OwnedHandle, expected: "directory" | "file"): void {
  const info = Buffer.alloc(8);
  if (!api.GetFileInformationByHandleEx(handle.borrow(), 9, info, 8)) throw failureForLastError();
  const attributes = info.readUInt32LE(0);
  if ((attributes & ATTRIBUTES.reparsePoint) !== 0) {
    throw new NativeFailure("reparse_point");
  }
  const directory = (attributes & ATTRIBUTES.directory) !== 0;
  if (directory !== (expected === "directory")) {
    throw new NativeFailure(expected === "directory" ? "not_directory" : "wrong_type");
  }
}
