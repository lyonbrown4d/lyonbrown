import { opendirSync } from "node:fs";

import type { NativeModelBundleIdentity } from "@lyonbrown/control-plane-native/internal/model-bundle-storage";
import {
  openDirectoryAt,
  openRegularAt,
  type PosixDirectory,
} from "@lyonbrown/control-plane-native/internal/posix/directory";
import {
  closeFd,
  fdPath,
  fsyncFd,
  unlinkAt,
} from "@lyonbrown/control-plane-native/internal/posix/native";
import {
  assertFile,
  assertName,
  fileStamp,
  missing,
  quiet,
  required,
} from "@lyonbrown/control-plane-native/internal/posix/model-shared";

interface BundleFile {
  readonly fd: number;
  readonly directory: PosixDirectory;
  readonly name: string;
  readonly stamp: string;
  readonly maximum: number;
}

/** File count and depth are bounded by the validated manifest (32 files, 256 bytes per path). */
export class ModelBundleTree {
  readonly #directories = new Map<string, PosixDirectory>();
  readonly #files: BundleFile[] = [];

  public constructor(
    public readonly directory: PosixDirectory,
    private readonly identity: NativeModelBundleIdentity,
    create: boolean,
  ) {
    this.#directories.set("", directory);
    try {
      const names = new Set(
        identity.files.flatMap((file) => {
          const parts = file.path.split("/").slice(0, -1);
          return parts.map((_, index) => parts.slice(0, index + 1).join("/"));
        }),
      );
      for (const name of [...names].sort((a, b) => a.split("/").length - b.split("/").length)) {
        const parts = name.split("/");
        const leaf = required(parts.pop());
        this.#directories.set(
          name,
          openDirectoryAt(required(this.#directories.get(parts.join("/"))), leaf, create),
        );
      }
    } catch (error) {
      this.close();
      throw error;
    }
  }

  public get callbackPath(): string {
    return process.platform === "linux" ? fdPath(this.directory.fd) : this.directory.path;
  }

  public seal(): void {
    if (this.#files.length !== 0) {
      this.validate();
      return;
    }
    try {
      this.#assertEntries();
      for (const file of this.identity.files) {
        const parts = file.path.split("/");
        const name = required(parts.pop());
        const directory = required(this.#directories.get(parts.join("/")));
        const fd = openRegularAt(directory, name);
        try {
          this.#files.push({
            fd,
            directory,
            name,
            stamp: fileStamp(fd, file.maxBytes),
            maximum: file.maxBytes,
          });
        } catch (error) {
          closeFd(fd);
          throw error;
        }
      }
    } catch (error) {
      for (const file of this.#files.splice(0))
        quiet(() => {
          closeFd(file.fd);
        });
      throw error;
    }
  }

  public validate(): void {
    this.#assertEntries();
    for (const file of this.#files) {
      assertName(file.directory, file.name, file.fd);
      assertFile(file.fd, file.stamp, file.maximum);
    }
  }

  public sync(): void {
    for (const file of this.#files) fsyncFd(file.fd);
    for (const directory of [...this.#directories.values()].toReversed()) fsyncFd(directory.fd);
  }

  public removeFiles(): void {
    // No recursive deletion: only manifest leaves and held private directory identities are removed.
    for (const file of this.identity.files) {
      const parts = file.path.split("/");
      const name = required(parts.pop());
      const directory = required(this.#directories.get(parts.join("/")));
      try {
        const fd = openRegularAt(directory, name);
        try {
          const anchored = this.#files.find(
            (file) => file.directory === directory && file.name === name,
          );
          if (anchored) assertName(directory, name, anchored.fd);
        } finally {
          closeFd(fd);
        }
        directory.assertIdentity();
        unlinkAt(directory.fd, name);
      } catch (error) {
        if (!missing(error)) throw error;
      }
    }
    for (const [name, directory] of [...this.#directories].toReversed()) {
      if (name === "") continue;
      directory.assertIdentity();
      const parts = name.split("/");
      const leaf = required(parts.pop());
      unlinkAt(required(this.#directories.get(parts.join("/"))).fd, leaf, true);
    }
  }

  public close(): void {
    for (const file of this.#files.splice(0))
      quiet(() => {
        closeFd(file.fd);
      });
    for (const directory of [...this.#directories.values()].toReversed())
      quiet(() => {
        directory.close();
      });
    this.#directories.clear();
  }

  #assertEntries(): void {
    for (const [name, directory] of this.#directories) {
      directory.assertIdentity();
      const prefix = name === "" ? "" : `${name}/`;
      const expected = new Set(
        this.identity.files
          .filter((file) => file.path.startsWith(prefix))
          .map((file) => file.path.slice(prefix.length).split("/")[0] ?? ""),
      );
      const path = process.platform === "linux" ? fdPath(directory.fd) : directory.path;
      const listing = opendirSync(path);
      let count = 0;
      try {
        for (let entry = listing.readSync(); entry !== null; entry = listing.readSync()) {
          count += 1;
          if (count > expected.size || !expected.has(entry.name))
            throw new Error("native_model_storage_failed");
        }
        if (count !== expected.size) throw new Error("native_model_storage_failed");
      } finally {
        listing.closeSync();
      }
      directory.assertIdentity();
    }
  }
}
