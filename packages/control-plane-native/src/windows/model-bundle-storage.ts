import { randomBytes } from "node:crypto";
import { win32 } from "node:path";

import type {
  NativeInstalledBundleResult,
  NativeModelBundleIdentity,
  NativeModelBundleInstallLock,
  NativeModelBundleStorageCapability,
  NativeModelStagingBundle,
} from "@lyonbrown/control-plane-native/internal/model-bundle-storage";
import { WindowsInstallLockAnchor } from "@lyonbrown/control-plane-native/internal/windows/model-artifacts";
import { WindowsModelStagingBundle } from "@lyonbrown/control-plane-native/internal/windows/model-bundle-artifacts";
import {
  assertActiveBundleDirectoryName,
  assertBundleOperationId,
  assertModelBundleIdentity,
  bundleIndexName,
  modelBundleKey,
  stagingBundleName,
} from "@lyonbrown/control-plane-native/internal/windows/model-bundle-identity";
import { readPrivateFile } from "@lyonbrown/control-plane-native/internal/windows/files";
import {
  createPrivateChildDirectory,
  openModelsDirectory,
  validateModelsDirectory,
  type ModelDirectoryLease,
} from "@lyonbrown/control-plane-native/internal/windows/model-directory";
import {
  assertSameFile,
  fileSize,
  openInstalledModel,
  openOrCreatePrivateLockFile,
} from "@lyonbrown/control-plane-native/internal/windows/model-files";
import {
  openDirectory,
  validateObjectType,
  type OwnedHandle,
} from "@lyonbrown/control-plane-native/internal/windows/handles";
import { fileIdentity } from "@lyonbrown/control-plane-native/internal/windows/identity";
import {
  currentUser,
  verifyPrivateObject,
} from "@lyonbrown/control-plane-native/internal/windows/security";
import {
  child,
  closeQuietly,
  promiseFrom,
  storageFailure,
  throwIfAborted,
} from "@lyonbrown/control-plane-native/internal/windows/model-storage-shared";
import { NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

interface AnchoredBundle {
  readonly directories: readonly OwnedHandle[];
  readonly files: readonly { handle: OwnedHandle; identity: string; maximumBytes: number }[];
}

export class WindowsModelBundleStorageCapability implements NativeModelBundleStorageCapability {
  readonly #modelsDirectory: string;
  #directoryIdentity: string | undefined;

  public constructor(modelsDirectory: string) {
    this.#modelsDirectory = validateModelsDirectory(modelsDirectory);
  }

  public acquireInstallLockAnchor(
    identity: NativeModelBundleIdentity,
    signal: AbortSignal,
  ): Promise<NativeModelBundleInstallLock> {
    return promiseFrom(() => {
      assertModelBundleIdentity(identity);
      throwIfAborted(signal);
      const directory = this.#openDirectory();
      try {
        const path = child(directory.canonicalPath, `.install-bundle-${modelBundleKey(identity)}`);
        return new WindowsInstallLockAnchor(directory, openOrCreatePrivateLockFile(path), path);
      } catch (error) {
        directory.close();
        throw storageFailure(error);
      }
    });
  }

  public createStagingBundle(
    identity: NativeModelBundleIdentity,
    operationId: string,
    signal: AbortSignal,
  ): Promise<NativeModelStagingBundle> {
    return promiseFrom(() => {
      assertModelBundleIdentity(identity);
      assertBundleOperationId(operationId);
      throwIfAborted(signal);
      const root = this.#openDirectory();
      const stagingPath = child(
        root.canonicalPath,
        stagingBundleName(identity, randomBytes(16).toString("hex")),
      );
      let staging: OwnedHandle | undefined;
      const children: OwnedHandle[] = [];
      try {
        staging = createPrivateChildDirectory(root, stagingPath);
        for (const relative of bundleDirectories(identity)) {
          children.push(createPrivateChildDirectory(root, win32.join(stagingPath, ...relative)));
        }
        return new WindowsModelStagingBundle(root, staging, children, stagingPath, identity);
      } catch (error) {
        for (const handle of children.toReversed()) closeQuietly(handle);
        closeQuietly(staging);
        root.close();
        throw storageFailure(error);
      }
    });
  }

  public async withInstalledBundle<T>(
    identity: NativeModelBundleIdentity,
    signal: AbortSignal,
    use: (directoryPath: string) => Promise<T>,
  ): Promise<NativeInstalledBundleResult<T>> {
    assertModelBundleIdentity(identity);
    throwIfAborted(signal);
    const root = this.#openDirectory();
    let directoryName: string | undefined;
    try {
      directoryName = readActiveDirectoryName(root.canonicalPath, identity);
    } catch (error) {
      root.close();
      throw storageFailure(error);
    }
    if (directoryName === undefined) {
      root.close();
      return { kind: "missing" };
    }
    const path = child(root.canonicalPath, directoryName);
    let anchored: AnchoredBundle | undefined;
    try {
      try {
        anchored = anchorBundle(path, identity);
      } catch (error) {
        if (error instanceof NativeFailure && error.failureCode === "not_found") {
          return { kind: "missing" };
        }
        throw error;
      }
      const value = await use(path);
      throwIfAborted(signal);
      validateAnchored(anchored);
      return { kind: "present", value };
    } catch (error) {
      throw storageFailure(error);
    } finally {
      closeAnchored(anchored);
      root.close();
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

function readActiveDirectoryName(
  root: string,
  identity: NativeModelBundleIdentity,
): string | undefined {
  const bytes = readPrivateFile(child(root, bundleIndexName(identity)), 512);
  if (bytes === null) return undefined;
  let value: unknown;
  try {
    value = JSON.parse(Buffer.from(bytes).toString("utf8"));
  } catch {
    throw new Error("native_model_bundle_identity_invalid");
  }
  if (
    typeof value !== "object" ||
    value === null ||
    Array.isArray(value) ||
    Object.keys(value).length !== 2 ||
    !("version" in value) ||
    !("directory" in value) ||
    value.version !== 1
  ) {
    throw new Error("native_model_bundle_identity_invalid");
  }
  return assertActiveBundleDirectoryName(identity, value.directory);
}

function bundleDirectories(identity: NativeModelBundleIdentity): readonly string[][] {
  const directories = new Map<string, string[]>();
  for (const file of identity.files) {
    const segments = file.path.split("/").slice(0, -1);
    for (let length = 1; length <= segments.length; length += 1) {
      const value = segments.slice(0, length);
      directories.set(value.join("/"), value);
    }
  }
  return [...directories.values()].sort((left, right) => left.length - right.length);
}

function anchorBundle(path: string, identity: NativeModelBundleIdentity): AnchoredBundle {
  const directories: OwnedHandle[] = [];
  const files: { handle: OwnedHandle; identity: string; maximumBytes: number }[] = [];
  try {
    for (const relative of [[], ...bundleDirectories(identity)]) {
      const handle = openDirectory(win32.join(path, ...relative), true);
      validateObjectType(handle, "directory");
      verifyPrivateObject(handle.borrow(), currentUser(), true);
      directories.push(handle);
    }
    for (const file of identity.files) {
      const handle = openInstalledModel(win32.join(path, ...file.path.split("/")));
      fileSize(handle, file.maxBytes);
      files.push({ handle, identity: fileIdentity(handle), maximumBytes: file.maxBytes });
    }
    return { directories, files };
  } catch (error) {
    closeAnchored({ directories, files });
    throw error;
  }
}

function validateAnchored(bundle: AnchoredBundle): void {
  for (const file of bundle.files) {
    assertSameFile(file.handle, file.identity);
    fileSize(file.handle, file.maximumBytes);
  }
}

function closeAnchored(bundle: AnchoredBundle | undefined): void {
  if (bundle === undefined) return;
  for (const file of bundle.files.toReversed()) closeQuietly(file.handle);
  for (const directory of bundle.directories.toReversed()) closeQuietly(directory);
}

export function createWindowsModelBundleStorageCapability(
  modelsDirectory: string,
): NativeModelBundleStorageCapability {
  return new WindowsModelBundleStorageCapability(modelsDirectory);
}
