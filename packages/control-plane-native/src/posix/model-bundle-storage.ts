import { randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";

import type {
  NativeInstalledBundleResult,
  NativeModelBundleIdentity,
  NativeModelBundleInstallLock,
  NativeModelBundleStorageCapability,
  NativeModelStagingBundle,
} from "@lyonbrown/control-plane-native/internal/model-bundle-storage";
import {
  openDirectoryAt,
  openRegularAt,
  type PosixDirectory,
} from "@lyonbrown/control-plane-native/internal/posix/directory";
import { closeFd } from "@lyonbrown/control-plane-native/internal/posix/native";
import { PosixModelStagingBundle } from "@lyonbrown/control-plane-native/internal/posix/model-bundle-artifacts";
import { ModelBundleTree } from "@lyonbrown/control-plane-native/internal/posix/model-bundle-tree";
import {
  assertBundleIdentity,
  assertOperationId,
  bundleKey,
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

export class PosixModelBundleStorageCapability implements NativeModelBundleStorageCapability {
  readonly #root: ModelRoot;

  public constructor(path: string) {
    this.#root = new ModelRoot(path);
  }

  public acquireInstallLockAnchor(
    identity: NativeModelBundleIdentity,
    signal: AbortSignal,
  ): Promise<NativeModelBundleInstallLock> {
    return promiseFrom(() => {
      assertBundleIdentity(identity);
      cancelled(signal);
      let root: PosixDirectory | undefined;
      try {
        root = this.#root.open();
        const name = `.install-bundle-${bundleKey(identity)}`;
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

  public createStagingBundle(
    identity: NativeModelBundleIdentity,
    operationId: string,
    signal: AbortSignal,
  ): Promise<NativeModelStagingBundle> {
    return promiseFrom(() => {
      assertBundleIdentity(identity);
      assertOperationId(operationId);
      cancelled(signal);
      let root: PosixDirectory | undefined;
      let directory: PosixDirectory | undefined;
      try {
        root = this.#root.open();
        const key = bundleKey(identity);
        const name = `.staging-bundle-${key}-${randomBytes(16).toString("hex")}`;
        directory = openDirectoryAt(root, name, true);
        return new PosixModelStagingBundle(
          root,
          new ModelBundleTree(directory, identity, true),
          name,
          key,
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

  public async withInstalledBundle<T>(
    identity: NativeModelBundleIdentity,
    signal: AbortSignal,
    use: (path: string) => Promise<T>,
  ): Promise<NativeInstalledBundleResult<T>> {
    assertBundleIdentity(identity);
    cancelled(signal);
    let root: PosixDirectory | undefined;
    let tree: ModelBundleTree | undefined;
    let unlock: (() => void) | undefined;
    let index: number | undefined;
    try {
      root = this.#root.open();
      const key = bundleKey(identity);
      const indexName = `.bundle-index-${key}.json`;
      unlock = accessLock(root, key, false);
      try {
        index = openRegularAt(root, indexName);
      } catch (error) {
        if (missing(error)) return { kind: "missing" };
        throw error;
      }
      const stamp = fileStamp(index, 512);
      const name = activeDirectory(readFileSync(index, "utf8"), key);
      try {
        tree = new ModelBundleTree(openDirectoryAt(root, name), identity, false);
        tree.seal();
      } catch (error) {
        if (missing(error)) return { kind: "missing" };
        throw error;
      }
      let value: T;
      try {
        value = await use(tree.callbackPath);
      } catch (error) {
        throw new ModelCallbackFailure(error);
      } finally {
        tree.validate();
        assertName(root, indexName, index);
        assertFile(index, stamp, 512);
      }
      cancelled(signal);
      return { kind: "present", value };
    } catch (error) {
      if (error instanceof ModelCallbackFailure)
        throw error.error instanceof Error ? error.error : new Error("model_callback_failed");
      throw storageFailure(error);
    } finally {
      tree?.close();
      if (index !== undefined)
        quiet(() => {
          closeFd(required(index));
        });
      if (unlock) quiet(unlock);
      if (root)
        quiet(() => {
          required(root).close();
        });
    }
  }
}

function activeDirectory(text: string, key: string): string {
  const value: unknown = JSON.parse(text);
  const prefix = `.staging-bundle-${key}-`;
  if (
    typeof value !== "object" ||
    value === null ||
    Array.isArray(value) ||
    Object.keys(value).length !== 2 ||
    !("version" in value) ||
    value.version !== 1 ||
    !("directory" in value) ||
    typeof value.directory !== "string" ||
    !value.directory.startsWith(prefix) ||
    !/^[a-f0-9]{32}$/u.test(value.directory.slice(prefix.length))
  ) {
    throw new Error("native_model_bundle_identity_invalid");
  }
  return value.directory;
}

export function createPosixModelBundleStorageCapability(
  path: string,
): NativeModelBundleStorageCapability {
  return new PosixModelBundleStorageCapability(path);
}
