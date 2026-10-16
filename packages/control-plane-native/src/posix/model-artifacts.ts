import { join } from "node:path";

import type { NativeModelStagingFile } from "@lyonbrown/local-model/composition";
import {
  openRegularAt,
  type PosixDirectory,
} from "@lyonbrown/control-plane-native/internal/posix/directory";
import {
  closeFd,
  fdPath,
  fsyncFd,
  renameBetween,
  unlinkAt,
} from "@lyonbrown/control-plane-native/internal/posix/native";
import {
  accessLock,
  assertFile,
  assertName,
  fileStamp,
  missing,
  quiet,
  storageFailure,
  promiseFrom,
  required,
} from "@lyonbrown/control-plane-native/internal/posix/model-shared";

export class PosixModelStagingFile implements NativeModelStagingFile {
  #state: "writable" | "sealed" | "activated" | "cleaned" = "writable";
  #active = false;
  #file: number | undefined;
  #stamp = "";
  #cleanup: Promise<void> | undefined;

  public constructor(
    private readonly root: PosixDirectory,
    private readonly directory: PosixDirectory,
    private readonly stagingName: string,
    private readonly filename: string,
    private readonly key: string,
    private readonly maximum: number,
  ) {}

  public async withFilePath<T>(use: (file: string, directory: string) => Promise<T>): Promise<T> {
    if (this.#active || this.#state === "cleaned" || this.#state === "activated") {
      throw new Error("native_model_storage_failed");
    }
    this.#active = true;
    try {
      this.directory.assertIdentity();
      if (this.#state === "sealed") this.#validate();
      // Darwin fdesc cannot traverse directories. The private path is checked around the callback.
      const directoryPath =
        process.platform === "linux" ? fdPath(this.directory.fd) : this.directory.path;
      const path = join(directoryPath, this.filename);
      let value: T;
      try {
        value = await use(path, directoryPath);
      } finally {
        this.directory.assertIdentity();
        if (this.#state === "sealed") this.#validate();
      }
      if (this.#state === "writable") this.#seal();
      else this.#validate();
      return value;
    } finally {
      this.#active = false;
    }
  }

  public activate(): Promise<void> {
    return promiseFrom(() => {
      if (this.#state === "activated") return;
      if (this.#active || this.#state === "cleaned") throw new Error("native_model_storage_failed");
      let unlock: (() => void) | undefined;
      try {
        this.directory.assertIdentity();
        if (this.#state === "writable") this.#seal(true);
        this.#validate();
        unlock = accessLock(this.root, this.key, true);
        const destination = `model-${this.key}.gguf`;
        try {
          const existing = openRegularAt(this.root, destination);
          try {
            fileStamp(existing, this.maximum);
          } finally {
            closeFd(existing);
          }
        } catch (error) {
          if (!missing(error)) throw error;
        }
        fsyncFd(required(this.#file));
        renameBetween(this.directory.fd, this.filename, this.root.fd, destination);
        fsyncFd(this.directory.fd);
        fsyncFd(this.root.fd);
        assertName(this.root, destination, required(this.#file));
        this.#stamp = fileStamp(required(this.#file), this.maximum);
        this.#state = "activated";
      } catch (error) {
        throw storageFailure(error);
      } finally {
        if (unlock) quiet(unlock);
      }
    });
  }

  public cleanup(): Promise<void> {
    if (this.#active) return Promise.reject(new Error("native_model_storage_failed"));
    this.#cleanup ??= this.#dispose();
    return this.#cleanup;
  }

  #seal(create = false): void {
    const fd = openRegularAt(this.directory, this.filename, create);
    try {
      this.#stamp = fileStamp(fd, this.maximum);
      this.#file = fd;
      this.#state = "sealed";
    } catch (error) {
      closeFd(fd);
      throw error;
    }
  }

  #validate(): void {
    this.directory.assertIdentity();
    assertName(this.directory, this.filename, required(this.#file));
    assertFile(required(this.#file), this.#stamp, this.maximum);
  }

  #dispose(): Promise<void> {
    return promiseFrom(() => {
      const activated = this.#state === "activated";
      this.#state = "cleaned";
      let failure: unknown;
      try {
        this.directory.assertIdentity();
        if (!activated) {
          try {
            const current = openRegularAt(this.directory, this.filename);
            try {
              if (this.#file !== undefined) assertName(this.directory, this.filename, this.#file);
            } finally {
              closeFd(current);
            }
            unlinkAt(this.directory.fd, this.filename);
          } catch (error) {
            if (!missing(error)) throw error;
          }
        }
        unlinkAt(this.root.fd, this.stagingName, true);
        fsyncFd(this.root.fd);
      } catch (error) {
        failure = error;
      } finally {
        if (this.#file !== undefined)
          quiet(() => {
            closeFd(required(this.#file));
          });
        quiet(() => {
          this.directory.close();
        });
        quiet(() => {
          this.root.close();
        });
      }
      if (failure !== undefined) throw storageFailure(failure);
    });
  }
}
