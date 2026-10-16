import { randomBytes } from "node:crypto";
import { writeFileSync } from "node:fs";

import type { NativeModelStagingBundle } from "@lyonbrown/control-plane-native/internal/model-bundle-storage";
import {
  openRegularAt,
  type PosixDirectory,
} from "@lyonbrown/control-plane-native/internal/posix/directory";
import {
  closeFd,
  fsyncFd,
  renameAt,
  unlinkAt,
} from "@lyonbrown/control-plane-native/internal/posix/native";
import type { ModelBundleTree } from "@lyonbrown/control-plane-native/internal/posix/model-bundle-tree";
import {
  accessLock,
  missing,
  quiet,
  storageFailure,
  promiseFrom,
  required,
} from "@lyonbrown/control-plane-native/internal/posix/model-shared";

export class PosixModelStagingBundle implements NativeModelStagingBundle {
  #state: "writable" | "sealed" | "activated" | "cleaned" = "writable";
  #active = false;
  #cleanup: Promise<void> | undefined;

  public constructor(
    private readonly root: PosixDirectory,
    private readonly tree: ModelBundleTree,
    private readonly name: string,
    private readonly key: string,
  ) {}

  public async withDirectoryPath<T>(use: (path: string) => Promise<T>): Promise<T> {
    if (this.#active || this.#state === "cleaned") throw new Error("native_model_storage_failed");
    this.#active = true;
    let unlock: (() => void) | undefined;
    try {
      this.tree.directory.assertIdentity();
      if (this.#state !== "writable") this.tree.validate();
      if (this.#state === "activated") unlock = accessLock(this.root, this.key, false);
      let value: T;
      try {
        value = await use(this.tree.callbackPath);
      } finally {
        this.tree.directory.assertIdentity();
        if (this.#state !== "writable") this.tree.validate();
      }
      if (this.#state === "writable") {
        this.tree.seal();
        this.#state = "sealed";
      } else this.tree.validate();
      return value;
    } finally {
      if (unlock) quiet(unlock);
      this.#active = false;
    }
  }

  public activate(): Promise<void> {
    return promiseFrom(() => {
      if (this.#state === "activated") return;
      if (this.#active || this.#state === "cleaned") throw new Error("native_model_storage_failed");
      let unlock: (() => void) | undefined;
      const temporary = `.bundle-index-${this.key}-${randomBytes(16).toString("hex")}.tmp`;
      let fd: number | undefined;
      try {
        this.tree.seal();
        this.tree.validate();
        unlock = accessLock(this.root, this.key, true);
        this.tree.sync();
        try {
          const existing = openRegularAt(this.root, `.bundle-index-${this.key}.json`);
          closeFd(existing);
        } catch (error) {
          if (!missing(error)) throw error;
        }
        fd = openRegularAt(this.root, temporary, true, true);
        writeFileSync(fd, JSON.stringify({ version: 1, directory: this.name }));
        fsyncFd(fd);
        this.root.assertIdentity();
        renameAt(this.root.fd, temporary, `.bundle-index-${this.key}.json`);
        fsyncFd(this.root.fd);
        this.tree.validate();
        this.#state = "activated";
      } catch (error) {
        throw storageFailure(error);
      } finally {
        if (fd !== undefined)
          quiet(() => {
            closeFd(required(fd));
          });
        quiet(() => {
          try {
            unlinkAt(this.root.fd, temporary);
          } catch (error) {
            if (!missing(error)) throw error;
          }
        });
        if (unlock) quiet(unlock);
      }
    });
  }

  public cleanup(): Promise<void> {
    if (this.#active) return Promise.reject(new Error("native_model_storage_failed"));
    this.#cleanup ??= this.#dispose();
    return this.#cleanup;
  }

  #dispose(): Promise<void> {
    return promiseFrom(() => {
      const activated = this.#state === "activated";
      this.#state = "cleaned";
      let failure: unknown;
      try {
        if (!activated) {
          this.tree.directory.assertIdentity();
          this.tree.removeFiles();
          unlinkAt(this.root.fd, this.name, true);
          fsyncFd(this.root.fd);
        }
      } catch (error) {
        failure = error;
      } finally {
        this.tree.close();
        quiet(() => {
          this.root.close();
        });
      }
      if (failure !== undefined) throw storageFailure(failure);
    });
  }
}
