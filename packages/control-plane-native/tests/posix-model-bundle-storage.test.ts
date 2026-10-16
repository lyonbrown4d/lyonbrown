import { mkdtemp, readFile, readdir, realpath, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { createPosixModelBundleStorageCapability } from "@lyonbrown/control-plane-native/internal/posix/model-bundle-storage";
import { createPosixModelStorageCapability } from "@lyonbrown/control-plane-native/internal/posix/model-storage";
import { bundleKey } from "@lyonbrown/control-plane-native/internal/posix/model-identity";

const identity = {
  modelId: "decision-model",
  revision: "a".repeat(40),
  files: [
    { path: "config.json", sha256: "b".repeat(64), maxBytes: 64 },
    { path: "onnx/model.onnx", sha256: "c".repeat(64), maxBytes: 64 },
  ],
} as const;
const roots: string[] = [];
const signal = () => new AbortController().signal;
const write = (path: string, value: string | Buffer) =>
  writeFile(path, value, { mode: 0o600, flag: "wx" });
const populate = async (path: string, value = "model") => {
  await write(join(path, "config.json"), "config");
  await write(join(path, "onnx", "model.onnx"), value);
};

async function modelsPath(): Promise<string> {
  const root = await mkdtemp(join(await realpath(tmpdir()), "lyonbrown-posix-bundle-"));
  roots.push(root);
  return join(root, "models");
}

afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { force: true, recursive: true });
});

describe.runIf(process.platform === "darwin" || process.platform === "linux")(
  "POSIX model bundle storage",
  () => {
    it("publishes a complete tree atomically and reuses it across adapter instances", async () => {
      const models = await modelsPath();
      const storage = createPosixModelBundleStorageCapability(models);
      await expect(
        storage.withInstalledBundle(identity, signal(), () => Promise.resolve(true)),
      ).resolves.toEqual({ kind: "missing" });
      const staging = await storage.createStagingBundle(identity, "install", signal());
      await staging.withDirectoryPath((path) => populate(path));
      await expect(
        staging.withDirectoryPath((path) => readFile(join(path, "onnx/model.onnx"), "utf8")),
      ).resolves.toBe("model");
      await staging.activate();
      await staging.activate();
      await staging.cleanup();
      await staging.cleanup();
      const cached = createPosixModelBundleStorageCapability(models);
      await expect(
        cached.withInstalledBundle(identity, signal(), (path) =>
          readFile(join(path, "onnx/model.onnx"), "utf8"),
        ),
      ).resolves.toEqual({ kind: "present", value: "model" });
      const other = {
        ...identity,
        files: identity.files.map((file) => ({ ...file, maxBytes: 65 })),
      };
      await expect(
        cached.withInstalledBundle(other, signal(), () => Promise.resolve(true)),
      ).resolves.toEqual({
        kind: "missing",
      });
    });

    it("rejects path collisions, traversal, oversize files, and partial bundles", async () => {
      const models = await modelsPath();
      const storage = createPosixModelBundleStorageCapability(models);
      for (const files of [
        [{ ...identity.files[0], path: "../escape" }],
        [{ ...identity.files[0], path: "ONNX/model.onnx" }, identity.files[1]],
        [{ ...identity.files[0], path: "onnx" }, identity.files[1]],
      ]) {
        await expect(
          storage.createStagingBundle({ ...identity, files }, "invalid", signal()),
        ).rejects.toThrow("native_model_bundle_identity_invalid");
      }
      const staging = await storage.createStagingBundle(identity, "partial", signal());
      await expect(
        staging.withDirectoryPath((path) => write(join(path, "config.json"), "partial")),
      ).rejects.toThrow("native_model_storage_failed");
      await staging.cleanup();
      const oversize = await storage.createStagingBundle(identity, "oversize", signal());
      await expect(
        oversize.withDirectoryPath(async (path) => {
          await write(join(path, "config.json"), "config");
          await write(join(path, "onnx/model.onnx"), Buffer.alloc(65));
        }),
      ).rejects.toThrow("native_model_file_oversize");
      await oversize.cleanup();
      expect((await readdir(models)).filter((name) => name.startsWith(".staging-"))).toEqual([]);
    });

    it("excludes publication during installed readers and protects callback lifecycle", async () => {
      const models = await modelsPath();
      const storage = createPosixModelBundleStorageCapability(models);
      const first = await storage.createStagingBundle(identity, "first", signal());
      await first.withDirectoryPath(async (path) => {
        await populate(path, "first");
        await expect(first.cleanup()).rejects.toThrow("native_model_storage_failed");
        await expect(first.activate()).rejects.toThrow("native_model_storage_failed");
        await expect(first.withDirectoryPath(() => Promise.resolve(true))).rejects.toThrow(
          "native_model_storage_failed",
        );
      });
      await first.activate();
      const second = await createPosixModelBundleStorageCapability(models).createStagingBundle(
        identity,
        "second",
        signal(),
      );
      await second.withDirectoryPath((path) => populate(path, "second"));
      await storage.withInstalledBundle(identity, signal(), async (path) => {
        await expect(second.activate()).rejects.toThrow("native_model_storage_failed");
        expect(await readFile(join(path, "onnx/model.onnx"), "utf8")).toBe("first");
      });
      await first.withDirectoryPath(async () => {
        await expect(second.activate()).rejects.toThrow("native_model_storage_failed");
      });
      await second.activate();
      await first.cleanup();
      await second.cleanup();
      await expect(
        storage.withInstalledBundle(identity, signal(), (path) =>
          readFile(join(path, "onnx/model.onnx"), "utf8"),
        ),
      ).resolves.toEqual({ kind: "present", value: "second" });
    });

    it("keeps broken optional bundles separate from ordinary model storage", async () => {
      const models = await modelsPath();
      const bundle = createPosixModelBundleStorageCapability(models);
      const staging = await bundle.createStagingBundle(identity, "broken", signal());
      await staging.withDirectoryPath((path) => populate(path));
      await staging.activate();
      await staging.cleanup();
      const index = join(models, `.bundle-index-${bundleKey(identity)}.json`);
      await writeFile(index, "not json");
      await expect(
        bundle.withInstalledBundle(identity, signal(), () => Promise.resolve(true)),
      ).rejects.toThrow("native_model_storage_failed");
      const model = createPosixModelStorageCapability(models);
      const modelIdentity = {
        modelId: "normal-model",
        revision: "a".repeat(40),
        sha256: "b".repeat(64),
        filename: "normal.gguf",
        maxBytes: 64,
      };
      const ordinary = await model.createStagingFile(modelIdentity, "ordinary", signal());
      await ordinary.withFilePath((path) => write(path, "normal"));
      await ordinary.activate();
      await ordinary.cleanup();
      await expect(
        model.withInstalledFile(modelIdentity, signal(), (path) => readFile(path, "utf8")),
      ).resolves.toEqual({ kind: "present", value: "normal" });
    });

    it("rejects symlink bundle files and detects post-seal modification", async () => {
      const models = await modelsPath();
      const storage = createPosixModelBundleStorageCapability(models);
      const staging = await storage.createStagingBundle(identity, "symlink", signal());
      await expect(
        staging.withDirectoryPath(async (path) => {
          await write(join(path, "config.json"), "safe");
          await symlink(join(path, "config.json"), join(path, "onnx/model.onnx"));
        }),
      ).rejects.toThrow();
      await expect(staging.cleanup()).rejects.toThrow("native_model_storage_failed");
      const changed = await storage.createStagingBundle(identity, "changed", signal());
      let path = "";
      await changed.withDirectoryPath(async (directory) => {
        path = directory;
        await populate(directory);
      });
      await writeFile(join(path, "config.json"), "modified");
      await expect(changed.activate()).rejects.toThrow("native_model_storage_failed");
      await changed.cleanup();
      await expect(
        storage.withInstalledBundle(identity, signal(), () => Promise.resolve(true)),
      ).resolves.toEqual({ kind: "missing" });
    });
  },
);
