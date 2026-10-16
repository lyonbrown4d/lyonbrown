import { fstatSync, fsyncSync } from "node:fs";
import koffi from "koffi";
import { NativeFailure } from "@lyonbrown/control-plane-native/internal/result";
import { verifyNoExtendedAcl } from "@lyonbrown/control-plane-native/internal/posix/acl";
import { posixConfiguration } from "@lyonbrown/control-plane-native/internal/posix/constants";

const configuration = posixConfiguration(process.platform);
export const FLAGS = configuration.flags;

export const LOCK = { shared: 1, exclusive: 2, nonBlocking: 4, unlock: 8 } as const;
const libc = koffi.load(configuration.libc);
const open = libc.func("openat", "int", ["int", "str", "int", "..."]);
const close = libc.func("int close(int fd)");
const mkdir = libc.func("int mkdirat(int fd, const char *name, uint32_t mode)");
const rename = libc.func("int renameat(int oldfd, const char *old, int newfd, const char *name)");
const unlink = libc.func("int unlinkat(int fd, const char *name, int flags)");
const nativeFlock = libc.func("int flock(int fd, int operation)");

export function failureForErrno(errno = koffi.errno()): NativeFailure {
  const errors = koffi.os.errno;
  if (errno === errors.ENOENT) return new NativeFailure("not_found");
  if (errno === errors.EACCES || errno === errors.EPERM) return new NativeFailure("access_denied");
  if (errno === errors.ELOOP) return new NativeFailure("reparse_point");
  if (errno === errors.ENOTDIR) return new NativeFailure("not_directory");
  return new NativeFailure("io");
}

export function validateName(name: string): void {
  if (!name || name === "." || name === ".." || /[/\0]/u.test(name)) {
    throw new NativeFailure("io");
  }
}

function nativeOpen(fd: number, name: string, flags: number, mode: number): number {
  const result = Number(open(fd, name, flags | FLAGS.closeOnExec, "uint32_t", mode));
  if (result < 0) throw failureForErrno();
  return result;
}

export function openRoot(): number {
  return nativeOpen(configuration.workingDirectoryFd, "/", FLAGS.readOnly | FLAGS.directory, 0);
}

export function openAt(fd: number, name: string, flags: number, mode = 0o600): number {
  validateName(name);
  return nativeOpen(fd, name, flags | FLAGS.noFollow, mode);
}

export function closeFd(fd: number): void {
  // Never retry close: EINTR semantics differ and the descriptor may already be reused.
  if (Number(close(fd)) < 0) throw failureForErrno();
}

export function mkdirAt(fd: number, name: string): void {
  validateName(name);
  if (Number(mkdir(fd, name, 0o700)) < 0 && koffi.errno() !== koffi.os.errno.EEXIST) {
    throw failureForErrno();
  }
}

export function renameAt(fd: number, source: string, destination: string): void {
  renameBetween(fd, source, fd, destination);
}

export function renameBetween(
  sourceFd: number,
  source: string,
  destinationFd: number,
  destination: string,
): void {
  validateName(source);
  validateName(destination);
  if (Number(rename(sourceFd, source, destinationFd, destination)) < 0) throw failureForErrno();
}

export function unlinkAt(fd: number, name: string, directory = false): void {
  validateName(name);
  if (Number(unlink(fd, name, directory ? configuration.removeDirectoryFlag : 0)) < 0) {
    throw failureForErrno();
  }
}

export function flock(fd: number, operation: number): boolean {
  if (Number(nativeFlock(fd, operation)) === 0) return true;
  const errno = koffi.errno();
  if (errno === koffi.os.errno.EWOULDBLOCK || errno === koffi.os.errno.EAGAIN) return false;
  throw failureForErrno(errno);
}

export function fsyncFd(fd: number): void {
  fsyncSync(fd);
}

/** Descriptor aliases can address open files; Darwin does not support child traversal. */
export function fdPath(fd: number): string {
  return `${configuration.descriptorRoot}/${String(fd)}`;
}

export function sameIdentity(first: number, second: number): boolean {
  const a = fstatSync(first, { bigint: true });
  const b = fstatSync(second, { bigint: true });
  return a.dev === b.dev && a.ino === b.ino;
}

export function verifyPrivate(fd: number, directory: boolean): void {
  const stat = fstatSync(fd, { bigint: true });
  if (directory ? !stat.isDirectory() : !stat.isFile()) {
    throw new NativeFailure(directory ? "not_directory" : "wrong_type");
  }
  if (!process.getuid || stat.uid !== BigInt(process.getuid())) {
    throw new NativeFailure("wrong_owner");
  }
  if ((stat.mode & 0o7777n) !== (directory ? 0o700n : 0o600n)) {
    throw new NativeFailure("insecure_mode");
  }
  if (!directory && stat.nlink !== 1n) throw new NativeFailure("wrong_type");
  verifyNoExtendedAcl(fd);
}
