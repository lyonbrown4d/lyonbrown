import { win32 } from "node:path";

import type {
  NativeModelBundleIdentity,
  NativeModelStagingBundle,
} from "@lyonbrown/control-plane-native/internal/model-bundle-storage";
import {
  atomicReplace,
  disposeOnClose,
} from "@lyonbrown/control-plane-native/internal/windows/files";
import type { OwnedHandle } from "@lyonbrown/control-plane-native/internal/windows/handles";
import { fileIdentity } from "@lyonbrown/control-plane-native/internal/windows/identity";
import {
  assertSameFile,
  deleteStagedModelIfPresent,
  fileSize,
  openInstalledModel,
  openSealedBundleFile,
} from "@lyonbrown/control-plane-native/internal/windows/model-files";
import type { ModelDirectoryLease } from "@lyonbrown/control-plane-native/internal/windows/model-directory";
import { bundleIndexName } from "@lyonbrown/control-plane-native/internal/windows/model-bundle-identity";
import {
  closeQuietly,
  promiseFrom,
  storageFailure,
} from "@lyonbrown/control-plane-native/internal/windows/model-storage-shared";

type BundleState = "writable" | "sealed" | "activated" | "cleaned";

interface AnchoredFile {
  readonly handle: OwnedHandle;
  readonly identity: string;
  readonly maximumBytes: number;
  readonly path: string;
}

export class WindowsModelStagingBundle implements NativeModelStagingBundle {
  #state: BundleState = "writable";
  #callbackActive = false;
  #files: AnchoredFile[] = [];
  #cleanupPromise: Promise<void> | undefined;

  public constructor(
    private readonly root: ModelDirectoryLease,
    private readonly stagingDirectory: OwnedHandle,
    private readonly childDirectories: readonly OwnedHandle[],
    private readonly stagingPath: string,
    private readonly identity: NativeModelBundleIdentity,
  ) {}

  public async withDirectoryPath<T>(use: (directoryPath: string) => Promise<T>): Promise<T> {
    if (this.#state === "cleaned" || this.#callbackActive) {
      throw new Error("native_model_storage_failed");
    }
    this.#callbackActive = true;
    try {
      const value = await use(this.stagingPath);
      try {
        if (this.#state === "writable") this.#seal();
        else this.#validate();
      } catch (error) {
        throw storageFailure(error);
      }
      return value;
    } finally {
      this.#callbackActive = false;
    }
  }

  public activate(): Promise<void> {
    return promiseFrom(() => {
      if (this.#state === "activated") return;
      if (this.#state === "cleaned" || this.#callbackActive) {
        throw new Error("native_model_storage_failed");
      }
      try {
        if (this.#state === "writable") this.#seal();
        this.#validate();
        atomicReplace(
          this.root.handle,
          this.root.canonicalPath,
          bundleIndexName(this.identity),
          Buffer.from(
            JSON.stringify({ version: 1, directory: win32.basename(this.stagingPath) }),
            "utf8",
          ),
        );
        this.#validate();
        this.#validatePublishedPaths();
        this.#state = "activated";
      } catch (error) {
        throw storageFailure(error);
      }
    });
  }

  public cleanup(): Promise<void> {
    if (this.#callbackActive) return Promise.reject(new Error("native_model_storage_failed"));
    this.#cleanupPromise ??= promiseFrom(() => {
      this.#cleanup();
    });
    return this.#cleanupPromise;
  }

  #seal(): void {
    const opened: AnchoredFile[] = [];
    try {
      for (const file of this.identity.files) {
        const handle = openSealedBundleFile(win32.join(this.stagingPath, ...file.path.split("/")));
        fileSize(handle, file.maxBytes);
        opened.push({
          handle,
          identity: fileIdentity(handle),
          maximumBytes: file.maxBytes,
          path: win32.join(this.stagingPath, ...file.path.split("/")),
        });
      }
      this.#files = opened;
      this.#state = "sealed";
    } catch (error) {
      for (const file of opened) closeQuietly(file.handle);
      throw error;
    }
  }

  #validate(): void {
    for (const file of this.#files) {
      assertSameFile(file.handle, file.identity);
      fileSize(file.handle, file.maximumBytes);
    }
  }

  #validatePublishedPaths(): void {
    for (const file of this.#files) {
      const published = openInstalledModel(file.path);
      try {
        assertSameFile(published, file.identity);
        fileSize(published, file.maximumBytes);
      } finally {
        published.close();
      }
    }
  }

  #cleanup(): void {
    const activated = this.#state === "activated";
    this.#state = "cleaned";
    let failure: unknown;
    for (const file of this.#files) {
      try {
        file.handle.close();
      } catch (error) {
        failure ??= error;
      }
    }
    if (!activated) {
      for (const file of this.identity.files) {
        try {
          deleteStagedModelIfPresent(win32.join(this.stagingPath, ...file.path.split("/")));
        } catch (error) {
          failure ??= error;
        }
      }
    }
    for (const directory of this.childDirectories.toReversed()) {
      try {
        if (!activated) disposeOnClose(directory);
      } catch (error) {
        failure ??= error;
      }
      try {
        directory.close();
      } catch (error) {
        failure ??= error;
      }
    }
    try {
      if (!activated) disposeOnClose(this.stagingDirectory);
    } catch (error) {
      failure ??= error;
    }
    try {
      this.stagingDirectory.close();
    } catch (error) {
      failure ??= error;
    }
    try {
      this.root.close();
    } catch (error) {
      failure ??= error;
    }
    if (failure !== undefined) throw storageFailure(failure);
  }
}
