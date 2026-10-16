import { fstatSync } from "node:fs";
import { join } from "node:path";

import type { NativeInstallLockAnchor } from "@lyonbrown/local-model/composition";
import {
  openDirectory,
  openRegularAt,
  type PosixDirectory,
} from "@lyonbrown/control-plane-native/internal/posix/directory";
import { closeFd, flock, LOCK } from "@lyonbrown/control-plane-native/internal/posix/native";
import { validateModelsPath } from "@lyonbrown/control-plane-native/internal/posix/model-identity";
import { NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

export function cancelled(signal: AbortSignal): void {
  if (signal.aborted) throw new Error("model_download_cancelled");
}

export function storageFailure(error: unknown): Error {
  if (
    error instanceof Error &&
    /^(?:native_model_(?:identity_invalid|operation_invalid|directory_invalid|bundle_identity_invalid|file_oversize)|model_download_cancelled)$/u.test(
      error.message,
    )
  )
    return error;
  return new Error("native_model_storage_failed");
}

export function missing(error: unknown): boolean {
  return error instanceof NativeFailure && error.failureCode === "not_found";
}

export function quiet(operation: () => void): void {
  try {
    operation();
  } catch {
    /* Preserve the primary operation result. */
  }
}

export function required<T>(value: T | undefined): T {
  if (value === undefined) throw new Error("native_model_storage_failed");
  return value;
}

export function promiseFrom<T>(operation: () => T | Promise<T>): Promise<T> {
  try {
    return Promise.resolve(operation());
  } catch (error) {
    return Promise.reject(
      error instanceof Error ? error : new Error("native_model_storage_failed"),
    );
  }
}

export class ModelCallbackFailure extends Error {
  public constructor(public readonly error: unknown) {
    super("model_callback_failed");
  }
}

export function fileStamp(fd: number, maximum: number): string {
  const stat = fstatSync(fd, { bigint: true });
  if (
    !stat.isFile() ||
    !process.getuid ||
    stat.uid !== BigInt(process.getuid()) ||
    (stat.mode & 0o077n) !== 0n ||
    stat.nlink !== 1n
  ) {
    throw new Error("native_model_storage_failed");
  }
  if (stat.size < 0n || stat.size > BigInt(maximum)) throw new Error("native_model_file_oversize");
  return [stat.dev, stat.ino, stat.size, stat.mtimeNs, stat.ctimeNs].map(String).join(":");
}

export function inode(fd: number): string {
  const stat = fstatSync(fd, { bigint: true });
  return [stat.dev, stat.ino].map(String).join(":");
}

export function assertFile(fd: number, stamp: string, maximum: number): void {
  if (fileStamp(fd, maximum) !== stamp) throw new Error("native_model_storage_failed");
}

export function assertName(directory: PosixDirectory, name: string, fd: number): void {
  directory.assertIdentity();
  const current = openRegularAt(directory, name);
  try {
    if (inode(current) !== inode(fd)) throw new Error("native_model_storage_failed");
  } finally {
    closeFd(current);
  }
}

export class ModelRoot {
  readonly #path: string;
  #identity: string | undefined;

  public constructor(path: string) {
    this.#path = validateModelsPath(path);
  }

  public open(): PosixDirectory {
    const directory = openDirectory(this.#path, true, true);
    try {
      const identity = inode(directory.fd);
      this.#identity ??= identity;
      if (this.#identity !== identity) throw new Error("native_model_storage_failed");
      return directory;
    } catch (error) {
      directory.close();
      throw error;
    }
  }
}

/** Advisory locks coordinate every adapter instance; they do not constrain hostile same-uid code. */
export function accessLock(root: PosixDirectory, key: string, exclusive: boolean): () => void {
  const fd = openRegularAt(root, `.access-${key}`, true);
  try {
    if (!flock(fd, (exclusive ? LOCK.exclusive : LOCK.shared) | LOCK.nonBlocking)) {
      throw new Error("native_model_storage_failed");
    }
    assertName(root, `.access-${key}`, fd);
  } catch (error) {
    closeFd(fd);
    throw error;
  }
  return () => {
    try {
      flock(fd, LOCK.unlock);
    } finally {
      closeFd(fd);
    }
  };
}

export class PosixInstallLockAnchor implements NativeInstallLockAnchor {
  #closed = false;
  #active = 0;

  public constructor(
    private readonly root: PosixDirectory,
    private readonly name: string,
    private readonly fd: number,
  ) {}

  public async withLockTarget<T>(use: (path: string) => Promise<T>): Promise<T> {
    if (this.#closed) throw new Error("native_model_storage_failed");
    this.#active += 1;
    try {
      assertName(this.root, this.name, this.fd);
      try {
        return await use(join(this.root.path, this.name));
      } finally {
        assertName(this.root, this.name, this.fd);
      }
    } finally {
      this.#active -= 1;
    }
  }

  public release(): Promise<void> {
    return promiseFrom(() => {
      if (this.#closed) return;
      if (this.#active > 0) throw new Error("native_model_storage_failed");
      this.#closed = true;
      try {
        closeFd(this.fd);
      } finally {
        this.root.close();
      }
    });
  }
}
