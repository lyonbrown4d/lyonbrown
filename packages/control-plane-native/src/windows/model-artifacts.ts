import type {
  NativeInstallLockAnchor,
  NativeModelStagingFile,
} from "@lyonbrown/local-model/composition";

import { SHARE } from "./constants.js";
import { atomicReplaceOpenFile, disposeOnClose } from "./files.js";
import type { OwnedHandle } from "./handles.js";
import type { ModelDirectoryLease } from "./model-directory.js";
import {
  assertSameFile,
  fileSize,
  flushModelFile,
  lockShared,
  openInstalledModel,
  reopenModelFile,
  type SharedFileLock,
} from "./model-files.js";
import {
  closeQuietly,
  promiseFrom,
  releaseQuietly,
  storageFailure,
} from "./model-storage-shared.js";
import { NativeFailure } from "../result.js";

export class WindowsInstallLockAnchor implements NativeInstallLockAnchor {
  #releaseStarted = false;
  #releasePromise: Promise<void> | undefined;

  public constructor(
    private readonly directory: ModelDirectoryLease,
    private readonly handle: OwnedHandle,
    private readonly path: string,
  ) {}

  public withLockTarget<T>(use: (lockTargetPath: string) => Promise<T>): Promise<T> {
    return promiseFrom(() => {
      if (this.#releaseStarted) throw new Error("native_model_storage_failed");
      this.handle.borrow();
      return use(this.path);
    });
  }

  public release(): Promise<void> {
    this.#releaseStarted = true;
    this.#releasePromise ??= promiseFrom(() => {
      let failure: unknown;
      try {
        this.handle.close();
      } catch (error) {
        failure = error;
      }
      try {
        this.directory.close();
      } catch (error) {
        failure ??= error;
      }
      if (failure !== undefined) throw storageFailure(failure);
    });
    return this.#releasePromise;
  }
}

type StagingState = "writable" | "sealed" | "activated" | "cleaned";

export class WindowsModelStagingFile implements NativeModelStagingFile {
  #state: StagingState = "writable";
  #callbackActive = false;
  #lock: SharedFileLock | undefined;
  #cleanupPromise: Promise<void> | undefined;

  public constructor(
    private readonly directory: ModelDirectoryLease,
    private readonly stagingDirectory: OwnedHandle,
    private file: OwnedHandle,
    private readonly filePath: string,
    private readonly stagingPath: string,
    private readonly destinationPath: string,
    private readonly destinationName: string,
    private readonly maximumBytes: number,
    private readonly initialIdentity: string,
  ) {}

  public async withFilePath<T>(
    use: (filePath: string, directoryPath: string) => Promise<T>,
  ): Promise<T> {
    if (this.#state === "cleaned" || this.#state === "activated" || this.#callbackActive) {
      throw new Error("native_model_storage_failed");
    }
    this.#callbackActive = true;
    try {
      this.#validate();
      return await use(this.filePath, this.stagingPath);
    } finally {
      this.#callbackActive = false;
      try {
        this.#validate();
      } finally {
        if (this.#state === "writable") this.#seal();
      }
    }
  }

  public activate(): Promise<void> {
    return promiseFrom(() => {
      this.#activate();
    });
  }

  public cleanup(): Promise<void> {
    if (this.#callbackActive) return Promise.reject(new Error("native_model_storage_failed"));
    this.#cleanupPromise ??= promiseFrom(() => {
      this.#cleanup();
    });
    return this.#cleanupPromise;
  }

  #activate(): void {
    if (this.#state === "activated") return;
    if (this.#state === "cleaned" || this.#callbackActive) {
      throw new Error("native_model_storage_failed");
    }
    try {
      if (this.#state === "writable") this.#seal();
      this.#validate();
      this.#validateExistingDestination();
      flushModelFile(this.file);
      atomicReplaceOpenFile(this.directory.handle, this.file, this.destinationName);
      const transition = reopenModelFile(this.file, SHARE.read | SHARE.write | SHARE.delete);
      this.#replaceAnchor(transition, lockShared(transition));
      const installed = reopenModelFile(this.file, SHARE.read);
      this.#replaceAnchor(installed, lockShared(installed));
      this.#assertInstalledPath();
      this.#state = "activated";
    } catch (error) {
      throw storageFailure(error);
    }
  }

  #cleanup(): void {
    if (this.#state === "cleaned") return;
    const activated = this.#state === "activated";
    this.#state = "cleaned";
    let failure: unknown;
    try {
      this.#lock?.release();
    } catch (error) {
      failure = error;
    }
    if (!activated) {
      try {
        disposeOnClose(this.file);
      } catch (error) {
        failure ??= error;
      }
    }
    const operations = [
      () => {
        this.file.close();
      },
      () => {
        disposeOnClose(this.stagingDirectory);
      },
      () => {
        this.stagingDirectory.close();
      },
      () => {
        this.directory.close();
      },
    ];
    for (const operation of operations) {
      try {
        operation();
      } catch (error) {
        failure ??= error;
      }
    }
    if (failure !== undefined) throw storageFailure(failure);
  }

  #seal(): void {
    this.#lock = lockShared(this.file);
    this.#state = "sealed";
  }

  #validate(): void {
    assertSameFile(this.file, this.initialIdentity);
    fileSize(this.file, this.maximumBytes);
  }

  #validateExistingDestination(): void {
    let existing: OwnedHandle | undefined;
    try {
      existing = openInstalledModel(this.destinationPath);
      fileSize(existing, this.maximumBytes);
    } catch (error) {
      if (error instanceof NativeFailure && error.failureCode === "not_found") return;
      throw error;
    } finally {
      closeQuietly(existing);
    }
  }

  #assertInstalledPath(): void {
    const installed = openInstalledModel(this.destinationPath);
    try {
      assertSameFile(installed, this.initialIdentity);
    } finally {
      installed.close();
    }
  }

  #replaceAnchor(next: OwnedHandle, nextLock: SharedFileLock): void {
    let failure: unknown;
    try {
      this.#lock?.release();
    } catch (error) {
      failure = error;
    }
    try {
      this.file.close();
    } catch (error) {
      failure ??= error;
    }
    if (failure !== undefined) {
      releaseQuietly(nextLock);
      closeQuietly(next);
      throw storageFailure(failure);
    }
    this.file = next;
    this.#lock = nextLock;
  }
}
