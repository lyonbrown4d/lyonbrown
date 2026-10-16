import { randomBytes } from "node:crypto";
import { fstatSync, readSync, writeFileSync } from "node:fs";
import type { KernelFileLock, PlatformResult } from "@lyonbrown/local-control-plane/native-adapter";
import {
  type PosixDirectory,
  openRegularAt,
} from "@lyonbrown/control-plane-native/internal/posix/directory";
import {
  closeFd,
  FLAGS,
  flock,
  fsyncFd,
  LOCK,
  openAt,
  renameAt,
  sameIdentity,
  unlinkAt,
  verifyPrivate,
} from "@lyonbrown/control-plane-native/internal/posix/native";
import { capture, NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

function existingFile(directory: PosixDirectory, name: string): number | null {
  try {
    return openRegularAt(directory, name);
  } catch (error) {
    if (error instanceof NativeFailure && error.failureCode === "not_found") {
      directory.assertIdentity();
      return null;
    }
    throw error;
  }
}

function assertFileIdentity(directory: PosixDirectory, name: string, fd: number): void {
  const linked = openRegularAt(directory, name);
  try {
    if (!sameIdentity(fd, linked)) throw new NativeFailure("io");
  } finally {
    closeFd(linked);
  }
}

export function readPrivateFile(
  directory: PosixDirectory,
  name: string,
  maximumBytes: number,
): Uint8Array | null {
  if (!Number.isSafeInteger(maximumBytes) || maximumBytes < 0) throw new NativeFailure("io");
  directory.assertIdentity();
  const fd = existingFile(directory, name);
  if (fd === null) return null;
  try {
    const before = fstatSync(fd, { bigint: true });
    if (before.size > BigInt(maximumBytes)) throw new NativeFailure("oversize");
    const bytes = Buffer.alloc(Number(before.size) + 1);
    let length = 0;
    // The extra byte detects growth without ever reading an unbounded descriptor.
    while (length < bytes.length) {
      const count = readSync(fd, bytes, length, bytes.length - length, length);
      if (count === 0) break;
      length += count;
    }
    if (length > maximumBytes) throw new NativeFailure("oversize");
    const after = fstatSync(fd, { bigint: true });
    if (
      BigInt(length) !== before.size ||
      before.size !== after.size ||
      before.mtimeNs !== after.mtimeNs ||
      before.ctimeNs !== after.ctimeNs
    ) {
      throw new NativeFailure("io");
    }
    verifyPrivate(fd, false);
    assertFileIdentity(directory, name, fd);
    return bytes.subarray(0, length);
  } finally {
    closeFd(fd);
  }
}

export function atomicReplace(directory: PosixDirectory, name: string, bytes: Uint8Array): void {
  directory.assertIdentity();
  const existing = existingFile(directory, name);
  if (existing !== null) closeFd(existing);
  const temporary = `.agentd-${randomBytes(16).toString("hex")}.tmp`;
  const fd = openAt(directory.fd, temporary, FLAGS.readWrite | FLAGS.create | FLAGS.exclusive);
  let renamed = false;
  try {
    verifyPrivate(fd, false);
    writeFileSync(fd, bytes);
    fsyncFd(fd);
    verifyPrivate(fd, false);
    directory.assertIdentity();
    assertFileIdentity(directory, temporary, fd);
    renameAt(directory.fd, temporary, name);
    renamed = true;
    fsyncFd(directory.fd);
    assertFileIdentity(directory, name, fd);
  } finally {
    try {
      if (!renamed) unlinkAt(directory.fd, temporary);
    } finally {
      closeFd(fd);
    }
  }
}

class PosixKernelLock implements KernelFileLock {
  #release: Promise<PlatformResult<void>> | undefined;

  public constructor(private readonly fd: number) {}

  public release(): Promise<PlatformResult<void>> {
    this.#release ??= capture(() => {
      try {
        if (!flock(this.fd, LOCK.unlock)) throw new NativeFailure("io");
      } finally {
        closeFd(this.fd);
      }
    });
    return this.#release;
  }
}

export function tryLock(directory: PosixDirectory, name: string): KernelFileLock | null {
  const fd = openRegularAt(directory, name, true);
  let owned = true;
  try {
    assertFileIdentity(directory, name, fd);
    if (!flock(fd, LOCK.exclusive | LOCK.nonBlocking)) {
      owned = false;
      closeFd(fd);
      return null;
    }
    assertFileIdentity(directory, name, fd);
    return new PosixKernelLock(fd);
  } catch (error) {
    if (owned) closeFd(fd);
    throw error;
  }
}
