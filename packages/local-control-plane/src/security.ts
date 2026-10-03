import { fchmod } from "node:fs";
import { chmod, lstat, mkdir } from "node:fs/promises";
import { parse, resolve } from "node:path";
import { promisify } from "node:util";
import { controlPlaneError, isNodeErrorWithCode } from "./errors.js";

export interface PosixMetadata {
  readonly mode: number;
  readonly uid: number;
  isDirectory(): boolean;
  isFile(): boolean;
  isSymbolicLink(): boolean;
}

export interface PosixFileSystem {
  chmodDirectory(path: string, mode: number): Promise<void>;
  chmodFile(path: string, fileDescriptor: number, mode: number): Promise<void>;
  lstat(path: string): Promise<PosixMetadata>;
}

const fchmodAsync = promisify(fchmod);
const defaultPosixFileSystem: PosixFileSystem = {
  chmodDirectory: chmod,
  chmodFile: (_path, fileDescriptor, mode) => fchmodAsync(fileDescriptor, mode),
  lstat,
};

export interface WindowsAclProtector {
  protectDirectory(path: string): Promise<void>;
  verifyDirectory(path: string): Promise<void>;
  protectFile(path: string, fileDescriptor: number): Promise<void>;
  verifyFile(path: string, fileDescriptor: number): Promise<void>;
}

export interface OpenedFileSecurity {
  readonly fileDescriptor: number;
  readonly metadata: PosixMetadata;
}

export interface SecurityPolicy {
  readonly platform: NodeJS.Platform;
  protectDirectory(path: string): Promise<void>;
  verifyDirectory(path: string): Promise<void>;
  protectFile(path: string, fileDescriptor: number): Promise<void>;
  verifyFile(path: string, opened: OpenedFileSecurity): Promise<void>;
}

class WindowsSecurityPolicy implements SecurityPolicy {
  readonly platform = "win32";
  constructor(private readonly protector: WindowsAclProtector) {}
  protectDirectory(path: string): Promise<void> {
    return this.protector.protectDirectory(path);
  }
  verifyDirectory(path: string): Promise<void> {
    return this.protector.verifyDirectory(path);
  }
  protectFile(path: string, fileDescriptor: number): Promise<void> {
    return this.protector.protectFile(path, fileDescriptor);
  }
  verifyFile(path: string, opened: OpenedFileSecurity): Promise<void> {
    return this.protector.verifyFile(path, opened.fileDescriptor);
  }
}

class PosixSecurityPolicy implements SecurityPolicy {
  constructor(
    readonly platform: NodeJS.Platform,
    private readonly effectiveUid: number,
    private readonly fileSystem: PosixFileSystem,
  ) {}

  async protectDirectory(path: string): Promise<void> {
    const metadata = await this.fileSystem.lstat(path);
    this.verifyKindAndOwner(metadata, "directory");
    await this.fileSystem.chmodDirectory(path, 0o700);
    await this.verify(path, "directory", 0o700);
  }
  async verifyDirectory(path: string): Promise<void> {
    await this.verify(path, "directory", 0o700);
  }
  async protectFile(path: string, fileDescriptor: number): Promise<void> {
    await this.fileSystem.chmodFile(path, fileDescriptor, 0o600);
  }
  verifyFile(path: string, opened: OpenedFileSecurity): Promise<void> {
    return Promise.resolve().then(() => {
      this.verifyMetadata(path, opened.metadata, "file", 0o600);
    });
  }
  private async verify(path: string, kind: "directory" | "file", expectedMode: number) {
    const metadata = await this.fileSystem.lstat(path);
    this.verifyMetadata(path, metadata, kind, expectedMode);
  }
  private verifyMetadata(
    _path: string,
    metadata: PosixMetadata,
    kind: "directory" | "file",
    expectedMode: number,
  ) {
    this.verifyKindAndOwner(metadata, kind);
    if ((metadata.mode & 0o777) !== expectedMode) throw controlPlaneError("INSECURE_PERMISSIONS");
  }
  private verifyKindAndOwner(metadata: PosixMetadata, kind: "directory" | "file") {
    if (metadata.isSymbolicLink()) throw controlPlaneError("SYMLINK_REJECTED");
    if (
      (kind === "directory" && !metadata.isDirectory()) ||
      (kind === "file" && !metadata.isFile())
    ) {
      throw controlPlaneError("WRONG_FILE_TYPE");
    }
    if (metadata.uid !== this.effectiveUid) throw controlPlaneError("INSECURE_OWNER");
  }
}

export interface SecurityOptions {
  readonly platform?: NodeJS.Platform;
  readonly windowsAclProtector?: WindowsAclProtector;
}

interface InternalSecurityOptions extends SecurityOptions {
  readonly posixEffectiveUid?: number;
  readonly posixFileSystem?: PosixFileSystem;
}

export function createSecurityPolicy(options: InternalSecurityOptions): SecurityPolicy {
  const platform = options.platform ?? process.platform;
  if (platform === "win32") {
    if (options.windowsAclProtector === undefined) throw controlPlaneError("WINDOWS_ACL_REQUIRED");
    return new WindowsSecurityPolicy(options.windowsAclProtector);
  }
  const effectiveUid = options.posixEffectiveUid ?? process.geteuid?.();
  if (effectiveUid === undefined) throw controlPlaneError("INSECURE_OWNER");
  return new PosixSecurityPolicy(
    platform,
    effectiveUid,
    options.posixFileSystem ?? defaultPosixFileSystem,
  );
}

async function existingMetadata(path: string) {
  try {
    return await lstat(path);
  } catch (error) {
    if (isNodeErrorWithCode(error, "ENOENT")) return undefined;
    throw controlPlaneError("IO_FAILURE", error);
  }
}

export async function prepareSecureDirectory(
  path: string,
  policy: SecurityPolicy,
): Promise<string> {
  const absolute = resolve(path);
  const root = parse(absolute).root;
  const relative = absolute
    .slice(root.length)
    .split(/[\\/]+/u)
    .filter(Boolean);
  let current = root;
  let targetWasCreated = false;

  for (const [index, component] of relative.entries()) {
    current = resolve(current, component);
    const metadata = await existingMetadata(current);
    if (metadata?.isSymbolicLink()) throw controlPlaneError("SYMLINK_REJECTED");
    if (metadata !== undefined && !metadata.isDirectory())
      throw controlPlaneError("WRONG_FILE_TYPE");
    if (metadata === undefined) {
      try {
        await mkdir(current, { mode: 0o700 });
      } catch (error) {
        if (!isNodeErrorWithCode(error, "EEXIST")) throw controlPlaneError("IO_FAILURE", error);
      }
      await policy.protectDirectory(current);
      await policy.verifyDirectory(current);
      if (index === relative.length - 1) targetWasCreated = true;
    }
  }
  if (!targetWasCreated) await policy.verifyDirectory(absolute);
  return absolute;
}

export async function rejectLinkOrWrongType(
  path: string,
  expected: "file" | "directory",
): Promise<void> {
  let metadata;
  try {
    metadata = await lstat(path);
  } catch (error) {
    if (isNodeErrorWithCode(error, "ENOENT")) throw controlPlaneError("DESCRIPTOR_NOT_FOUND");
    throw controlPlaneError("IO_FAILURE", error);
  }
  if (metadata.isSymbolicLink()) throw controlPlaneError("SYMLINK_REJECTED");
  if (
    (expected === "file" && !metadata.isFile()) ||
    (expected === "directory" && !metadata.isDirectory())
  ) {
    throw controlPlaneError("WRONG_FILE_TYPE");
  }
}
