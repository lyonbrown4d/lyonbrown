import { mkdtemp, readFile, realpath, rename, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { createPosixModelStorageCapability } from "@lyonbrown/control-plane-native/internal/posix/model-storage";
import { createPosixModelBundleStorageCapability } from "@lyonbrown/control-plane-native/internal/posix/model-bundle-storage";
import { bundleKey } from "@lyonbrown/control-plane-native/internal/posix/model-identity";

const roots: string[] = [];
const model = {
  modelId: "model",
  revision: "a".repeat(40),
  sha256: "b".repeat(64),
  filename: "model.gguf",
  maxBytes: 64,
};
const bundle = {
  modelId: "bundle",
  revision: model.revision,
  files: [{ path: "model.onnx", sha256: model.sha256, maxBytes: 64 }],
};
const signal = () => new AbortController().signal;
const write = (path: string, text: string) => writeFile(path, text, { flag: "wx", mode: 0o600 });

async function path(): Promise<string> {
  const root = await mkdtemp(join(await realpath(tmpdir()), "lyonbrown-model-boundary-"));
  roots.push(root);
  return join(root, "models");
}

afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { force: true, recursive: true });
});

describe.runIf(process.platform === "darwin" || process.platform === "linux")(
  "POSIX model storage boundaries",
  () => {
    it("validates portable path spellings and Unicode identities", async () => {
      for (const directory of [
        "relative",
        "/",
        "/private/tmp/../models",
        "/private/tmp/./models",
        "/private/tmp/bad\0name",
        "/private/tmp/bad\ud800",
      ]) {
        expect(() => createPosixModelStorageCapability(directory)).toThrow(
          "native_model_directory_invalid",
        );
      }
      const storage = createPosixModelStorageCapability(await path());
      for (const filename of ["bad\ud800", "bad\udc00", "bad\u0001", "CON", "model.", ""]) {
        await expect(
          storage.acquireInstallLockAnchor({ ...model, filename }, signal()),
        ).rejects.toThrow("native_model_identity_invalid");
      }
      const anchor = await storage.acquireInstallLockAnchor(
        { ...model, filename: "模型😀.gguf" },
        signal(),
      );
      await anchor.release();
    });

    it("supports empty activation, unused cleanup, and verifies installed-name drift", async () => {
      const storage = createPosixModelStorageCapability(await path());
      const unused = await storage.createStagingFile(model, "unused", signal());
      await unused.cleanup();
      const staging = await storage.createStagingFile(model, "empty", signal());
      await staging.activate();
      await staging.cleanup();
      await expect(
        storage.withInstalledFile(model, signal(), (file) => readFile(file)),
      ).resolves.toEqual({ kind: "present", value: Buffer.alloc(0) });
      await expect(
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- Untrusted callbacks may reject with non-Error values.
        storage.withInstalledFile(model, signal(), () => Promise.reject("opaque")),
      ).rejects.toThrow("model_callback_failed");
      await expect(
        storage.withInstalledFile(model, signal(), async (file) => {
          await rename(file, `${file}.moved`);
          await write(file, "replacement");
          return true;
        }),
      ).rejects.toThrow("native_model_storage_failed");
      await expect(staging.withFilePath(() => Promise.resolve(true))).rejects.toThrow(
        "native_model_storage_failed",
      );
    });

    it("keeps install targets bound and rejects symlink lock files", async () => {
      const models = await path();
      const storage = createPosixModelStorageCapability(models);
      const anchor = await storage.acquireInstallLockAnchor(model, signal());
      await expect(
        anchor.withLockTarget(async (target) => {
          await rename(target, `${target}.old`);
          await symlink(`${target}.old`, target);
        }),
      ).rejects.toThrow();
      await anchor.release();
      await expect(storage.acquireInstallLockAnchor(model, signal())).rejects.toThrow(
        "native_model_storage_failed",
      );
    });

    it("handles bundle install lock ownership, cancellation, and invalid manifests", async () => {
      const storage = createPosixModelBundleStorageCapability(await path());
      const anchor = await storage.acquireInstallLockAnchor(bundle, signal());
      await anchor.withLockTarget(async (target) => {
        expect(await readFile(target)).toHaveLength(0);
        await expect(anchor.release()).rejects.toThrow("native_model_storage_failed");
      });
      await anchor.release();
      await anchor.release();
      const cancelled = AbortSignal.abort();
      await expect(storage.acquireInstallLockAnchor(bundle, cancelled)).rejects.toThrow(
        "model_download_cancelled",
      );
      await expect(storage.createStagingBundle(bundle, "cancelled", cancelled)).rejects.toThrow(
        "model_download_cancelled",
      );
      await expect(
        storage.withInstalledBundle(bundle, cancelled, () => Promise.resolve(true)),
      ).rejects.toThrow("model_download_cancelled");
      await expect(
        storage.createStagingBundle({ ...bundle, files: [] }, "empty", signal()),
      ).rejects.toThrow("native_model_bundle_identity_invalid");
      await expect(storage.createStagingBundle(bundle, "../bad", signal())).rejects.toThrow(
        "native_model_operation_invalid",
      );
    });

    it("preserves bundle verifier failure, cancellation, and detects callback mutation", async () => {
      const models = await path();
      const storage = createPosixModelBundleStorageCapability(models);
      const staging = await storage.createStagingBundle(bundle, "ready", signal());
      await staging.withDirectoryPath((directory) => write(join(directory, "model.onnx"), "model"));
      await staging.activate();
      await staging.cleanup();
      const failure = new Error("bundle verifier failed");
      await expect(
        storage.withInstalledBundle(bundle, signal(), () => Promise.reject(failure)),
      ).rejects.toBe(failure);
      await expect(
        // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- Untrusted callbacks may reject with non-Error values.
        storage.withInstalledBundle(bundle, signal(), () => Promise.reject("opaque")),
      ).rejects.toThrow("model_callback_failed");
      const controller = new AbortController();
      await expect(
        storage.withInstalledBundle(bundle, controller.signal, () => {
          controller.abort();
          return Promise.resolve(true);
        }),
      ).rejects.toThrow("model_download_cancelled");
      await expect(
        storage.withInstalledBundle(bundle, signal(), async (directory) => {
          await writeFile(join(directory, "model.onnx"), "changed");
          return true;
        }),
      ).rejects.toThrow("native_model_storage_failed");
      await expect(staging.withDirectoryPath(() => Promise.resolve(true))).rejects.toThrow(
        "native_model_storage_failed",
      );
      await expect(staging.activate()).rejects.toThrow("native_model_storage_failed");
    });

    it("rejects unsafe bundle pointers and reports missing published directory", async () => {
      const models = await path();
      const storage = createPosixModelBundleStorageCapability(models);
      const staging = await storage.createStagingBundle(bundle, "ready", signal());
      let directory = "";
      await staging.withDirectoryPath(async (value) => {
        directory = value;
        await write(join(value, "model.onnx"), "model");
      });
      await staging.activate();
      await staging.cleanup();
      await rm(directory, { recursive: true });
      await expect(
        storage.withInstalledBundle(bundle, signal(), () => Promise.resolve(true)),
      ).resolves.toEqual({ kind: "missing" });
      const index = join(models, `.bundle-index-${bundleKey(bundle)}.json`);
      await writeFile(index, JSON.stringify({ version: 1, directory: "../outside" }));
      await expect(
        storage.withInstalledBundle(bundle, signal(), () => Promise.resolve(true)),
      ).rejects.toThrow("native_model_bundle_identity_invalid");
      await rename(index, `${index}.old`);
      await symlink(`${index}.old`, index);
      await expect(
        storage.withInstalledBundle(bundle, signal(), () => Promise.resolve(true)),
      ).rejects.toThrow("native_model_storage_failed");
    });
  },
);
