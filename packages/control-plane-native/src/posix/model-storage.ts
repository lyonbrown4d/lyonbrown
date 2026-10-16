import { randomBytes } from "node:crypto";
import { join } from "node:path";

import type {
  NativeModelIdentity,
  NativeModelStorageCapability,
  NativeInstallLockAnchor,
  NativeInstalledFileResult,
  NativeModelStagingFile,
} from "@lyonbrown/local-model/composition";
import {
  openDirectoryAt,
  openRegularAt,
  type PosixDirectory,
} from "@lyonbrown/control-plane-native/internal/posix/directory";
import { closeFd, fdPath } from "@lyonbrown/control-plane-native/internal/posix/native";
import { PosixModelStagingFile } from "@lyonbrown/control-plane-native/internal/posix/model-artifacts";
import {
  assertModelIdentity,
  assertOperationId,
  modelKey,
} from "@lyonbrown/control-plane-native/internal/posix/model-identity";
import {
  accessLock,
  assertFile,
  assertName,
  cancelled,
  fileStamp,
  missing,
  ModelCallbackFailure,
  ModelRoot,
  PosixInstallLockAnchor,
  quiet,
  storageFailure,
  promiseFrom,
  required,
} from "@lyonbrown/control-plane-native/internal/posix/model-shared";

export class PosixModelStorageCapability implements NativeModelStorageCapability {
  readonly #root: ModelRoot;

  public constructor(path: string) {
    this.#root = new ModelRoot(path);
  }

  public acquireInstallLockAnchor(
    identity: NativeModelIdentity,
    signal: AbortSignal,
  ): Promise<NativeInstallLockAnchor> {
    return promiseFrom(() => {
      assertModelIdentity(identity);
      cancelled(signal);
      let root: PosixDirectory | undefined;
      try {
        root = this.#root.open();
        const name = `.install-${modelKey(identity)}`;
        return new PosixInstallLockAnchor(root, name, openRegularAt(root, name, true));
      } catch (error) {
        if (root)
          quiet(() => {
            required(root).close();
          });
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
      cancelled(signal);
      let root: PosixDirectory | undefined;
      let directory: PosixDirectory | undefined;
      try {
        root = this.#root.open();
        const key = modelKey(identity);
        const name = `.staging-${key}-${randomBytes(16).toString("hex")}`;
        directory = openDirectoryAt(root, name, true);
        return new PosixModelStagingFile(
          root,
          directory,
          name,
          identity.filename,
          key,
          identity.maxBytes,
        );
      } catch (error) {
        if (directory)
          quiet(() => {
            required(directory).close();
          });
        if (root)
          quiet(() => {
            required(root).close();
          });
        throw storageFailure(error);
      }
    });
  }

  public async withInstalledFile<T>(
    identity: NativeModelIdentity,
    signal: AbortSignal,
    use: (path: string) => Promise<T>,
  ): Promise<NativeInstalledFileResult<T>> {
    assertModelIdentity(identity);
    cancelled(signal);
    let root: PosixDirectory | undefined;
    let file: number | undefined;
    let unlock: (() => void) | undefined;
    try {
      root = this.#root.open();
      const key = modelKey(identity);
      const name = `model-${key}.gguf`;
      unlock = accessLock(root, key, false);
      try {
        file = openRegularAt(root, name);
      } catch (error) {
        if (missing(error)) return { kind: "missing" };
        throw error;
      }
      const stamp = fileStamp(file, identity.maxBytes);
      let value: T;
      const directoryPath = process.platform === "linux" ? fdPath(root.fd) : root.path;
      try {
        value = await use(join(directoryPath, name));
      } catch (error) {
        throw new ModelCallbackFailure(error);
      } finally {
        assertName(root, name, file);
        assertFile(file, stamp, identity.maxBytes);
      }
      cancelled(signal);
      return { kind: "present", value };
    } catch (error) {
      if (error instanceof ModelCallbackFailure)
        throw error.error instanceof Error ? error.error : new Error("model_callback_failed");
      throw storageFailure(error);
    } finally {
      if (file !== undefined)
        quiet(() => {
          closeFd(required(file));
        });
      if (unlock) quiet(unlock);
      if (root)
        quiet(() => {
          required(root).close();
        });
    }
  }
}

export function createPosixModelStorageCapability(path: string): NativeModelStorageCapability {
  return new PosixModelStorageCapability(path);
}
