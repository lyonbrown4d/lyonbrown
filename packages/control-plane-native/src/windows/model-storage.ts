import { randomBytes } from "node:crypto";

import type {
  NativeInstalledFileResult,
  NativeInstallLockAnchor,
  NativeModelIdentity,
  NativeModelStagingFile,
  NativeModelStorageCapability,
} from "@lyonbrown/local-model/composition";

import { fileIdentity } from "@lyonbrown/control-plane-native/internal/windows/identity";
import type { OwnedHandle } from "@lyonbrown/control-plane-native/internal/windows/handles";
import {
  WindowsInstallLockAnchor,
  WindowsModelStagingFile,
} from "@lyonbrown/control-plane-native/internal/windows/model-artifacts";
import {
  createPrivateChildDirectory,
  openModelsDirectory,
  validateModelsDirectory,
  type ModelDirectoryLease,
} from "@lyonbrown/control-plane-native/internal/windows/model-directory";
import {
  assertSameFile,
  createPrivateModelFile,
  fileSize,
  lockShared,
  openInstalledModel,
  openOrCreatePrivateLockFile,
  type SharedFileLock,
} from "@lyonbrown/control-plane-native/internal/windows/model-files";
import {
  assertModelIdentity,
  assertOperationId,
  identityKey,
} from "@lyonbrown/control-plane-native/internal/windows/model-identity";
import {
  child,
  closeQuietly,
  installedName,
  promiseFrom,
  releaseQuietly,
  storageFailure,
  throwIfAborted,
} from "@lyonbrown/control-plane-native/internal/windows/model-storage-shared";
import { NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

class ModelStorageCallbackFailure extends Error {
  public constructor(readonly callbackError: unknown) {
    super("native model callback failed");
  }
}

export class WindowsModelStorageCapability implements NativeModelStorageCapability {
  readonly #modelsDirectory: string;
  #directoryIdentity: string | undefined;

  public constructor(modelsDirectory: string) {
    this.#modelsDirectory = validateModelsDirectory(modelsDirectory);
  }

  public acquireInstallLockAnchor(
    identity: NativeModelIdentity,
    signal: AbortSignal,
  ): Promise<NativeInstallLockAnchor> {
    return promiseFrom(() => {
      assertModelIdentity(identity);
      throwIfAborted(signal);
      let directory: ModelDirectoryLease | undefined;
      let handle: OwnedHandle | undefined;
      try {
        directory = this.#openDirectory();
        const path = child(directory.canonicalPath, `.install-${identityKey(identity)}`);
        handle = openOrCreatePrivateLockFile(path);
        throwIfAborted(signal);
        return new WindowsInstallLockAnchor(directory, handle, path);
      } catch (error) {
        closeQuietly(handle);
        try {
          directory?.close();
        } catch {
          // Preserve the primary error.
        }
        throw storageFailure(error);
      }
    });
  }

  public createStagingFile(
    identity: NativeModelIdentity,
    operationId: string,
    signal: AbortSignal,
  ): Promise<NativeModelStagingFile> {
    return promiseFrom(() => {
      assertModelIdentity(identity);
      assertOperationId(operationId);
      throwIfAborted(signal);
      let directory: ModelDirectoryLease | undefined;
      let stagingDirectory: OwnedHandle | undefined;
      let file: OwnedHandle | undefined;
      try {
        directory = this.#openDirectory();
        const stagingPath = child(
          directory.canonicalPath,
          `.staging-${identityKey(identity)}-${randomBytes(16).toString("hex")}`,
        );
        stagingDirectory = createPrivateChildDirectory(directory, stagingPath);
        const filePath = child(stagingPath, identity.filename);
        file = createPrivateModelFile(filePath);
        const fileId = fileIdentity(file);
        throwIfAborted(signal);
        return new WindowsModelStagingFile(
          directory,
          stagingDirectory,
          file,
          filePath,
          stagingPath,
          child(directory.canonicalPath, installedName(identity)),
          installedName(identity),
          identity.maxBytes,
          fileId,
        );
      } catch (error) {
        closeQuietly(file);
        closeQuietly(stagingDirectory);
        try {
          directory?.close();
        } catch {
          // Preserve the primary error.
        }
        throw storageFailure(error);
      }
    });
  }

  public async withInstalledFile<T>(
    identity: NativeModelIdentity,
    signal: AbortSignal,
    use: (filePath: string) => Promise<T>,
  ): Promise<NativeInstalledFileResult<T>> {
    assertModelIdentity(identity);
    throwIfAborted(signal);
    let directory: ModelDirectoryLease | undefined;
    let file: OwnedHandle | undefined;
    let lock: SharedFileLock | undefined;
    try {
      directory = this.#openDirectory();
      const path = child(directory.canonicalPath, installedName(identity));
      try {
        file = openInstalledModel(path);
      } catch (error) {
        if (error instanceof NativeFailure && error.failureCode === "not_found") {
          return { kind: "missing" };
        }
        throw error;
      }
      const expected = fileIdentity(file);
      fileSize(file, identity.maxBytes);
      lock = lockShared(file);
      let value: T;
      try {
        value = await use(path);
      } catch (error) {
        throw new ModelStorageCallbackFailure(error);
      }
      throwIfAborted(signal);
      assertSameFile(file, expected);
      fileSize(file, identity.maxBytes);
      return { kind: "present", value };
    } catch (error) {
      if (error instanceof ModelStorageCallbackFailure) {
        throw error.callbackError instanceof Error
          ? error.callbackError
          : new Error("model_callback_failed");
      }
      throw storageFailure(error);
    } finally {
      releaseQuietly(lock);
      closeQuietly(file);
      try {
        directory?.close();
      } catch {
        // The main operation result remains authoritative.
      }
    }
  }

  #openDirectory(): ModelDirectoryLease {
    const directory = openModelsDirectory(this.#modelsDirectory);
    if (this.#directoryIdentity === undefined) this.#directoryIdentity = directory.identity;
    if (this.#directoryIdentity !== directory.identity) {
      directory.close();
      throw new Error("native_model_identity_drift");
    }
    return directory;
  }
}

export function createWindowsModelStorageCapability(
  modelsDirectory: string,
): NativeModelStorageCapability {
  return new WindowsModelStorageCapability(modelsDirectory);
}
