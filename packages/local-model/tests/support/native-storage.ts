import { randomUUID } from "node:crypto";
import { lstat, mkdir, rename, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

import type {
  NativeInstalledFileResult,
  NativeInstallLockAnchor,
  NativeModelIdentity,
  NativeModelStagingFile,
  NativeModelStorageCapability,
} from "../../src/native-storage.js";

export class TestNativeModelStorage implements NativeModelStorageCapability {
  readonly identities: NativeModelIdentity[] = [];

  constructor(readonly root: string) {}

  lockTarget(identity: NativeModelIdentity): string {
    return join(this.root, `.${identity.modelId}.install`);
  }

  installedPath(identity: NativeModelIdentity): string {
    return join(this.root, identity.filename);
  }

  async acquireInstallLockAnchor(
    identity: NativeModelIdentity,
    signal: AbortSignal,
  ): Promise<NativeInstallLockAnchor> {
    this.record(identity);
    throwIfAborted(signal);
    await mkdir(this.root, { recursive: true });
    const target = this.lockTarget(identity);
    await writeFile(target, "", { flag: "a" });
    return {
      withLockTarget: (use) => use(target),
      release: () => Promise.resolve(),
    };
  }

  async createStagingFile(
    identity: NativeModelIdentity,
    operationId: string,
    signal: AbortSignal,
  ): Promise<NativeModelStagingFile> {
    this.record(identity);
    throwIfAborted(signal);
    await mkdir(this.root, { recursive: true });
    const directoryPath = join(this.root, `.staging-${operationId}-${randomUUID()}`);
    await mkdir(directoryPath);
    const filePath = join(directoryPath, identity.filename);
    let active = true;
    return {
      withFilePath: async (use) => {
        if (!active) throw new Error("staging_anchor_closed");
        return use(filePath, directoryPath);
      },
      activate: async () => {
        if (!active) throw new Error("staging_anchor_closed");
        await rename(filePath, this.installedPath(identity));
      },
      cleanup: async () => {
        if (!active) return;
        active = false;
        await rm(directoryPath, { recursive: true, force: true });
      },
    };
  }

  async withInstalledFile<T>(
    identity: NativeModelIdentity,
    signal: AbortSignal,
    use: (filePath: string) => Promise<T>,
  ): Promise<NativeInstalledFileResult<T>> {
    this.record(identity);
    throwIfAborted(signal);
    const filePath = this.installedPath(identity);
    try {
      await lstat(filePath);
    } catch (error) {
      if (isNotFound(error)) return { kind: "missing" };
      throw error;
    }
    const value = await use(filePath);
    return { kind: "present", value };
  }

  private record(identity: NativeModelIdentity): void {
    this.identities.push({ ...identity });
  }
}

const throwIfAborted = (signal: AbortSignal): void => {
  if (signal.aborted) throw new Error("cancelled");
};

const isNotFound = (error: unknown): boolean =>
  typeof error === "object" && error !== null && "code" in error && error.code === "ENOENT";
