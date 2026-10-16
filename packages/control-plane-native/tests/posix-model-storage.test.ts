import { createHash } from "node:crypto";
import {
  chmod,
  mkdir,
  mkdtemp,
  lstat,
  readFile,
  readdir,
  realpath,
  rename,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { createPosixModelStorageCapability } from "@lyonbrown/control-plane-native/internal/posix/model-storage";
import { modelKey } from "@lyonbrown/control-plane-native/internal/posix/model-identity";

const roots: string[] = [];
const identity = {
  modelId: "model-one",
  revision: "a".repeat(40),
  sha256: "b".repeat(64),
  filename: "model.gguf",
  maxBytes: 64,
};
const signal = () => new AbortController().signal;
const write = (path: string, value: string | Buffer) =>
  writeFile(path, value, { mode: 0o600, flag: "wx" });

async function verifyRegularModelPath(path: string): Promise<string> {
  // Production verification requires a regular leaf; a bare fd alias fails lstat.
  const metadata = await lstat(path);
  expect(metadata.isFile()).toBe(true);
  expect(metadata.isSymbolicLink()).toBe(false);
  expect(path.endsWith(".gguf")).toBe(true);
  const bytes = await readFile(path);
  expect(createHash("sha256").update(bytes).digest("hex")).toBe(
    createHash("sha256").update("verified model").digest("hex"),
  );
  return bytes.toString("utf8");
}

async function modelsPath(): Promise<string> {
  const root = await mkdtemp(join(await realpath(tmpdir()), "lyonbrown-posix-model-"));
  roots.push(root);
  return join(root, "models");
}

afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { force: true, recursive: true });
});

describe.runIf(process.platform === "darwin" || process.platform === "linux")(
  "POSIX model storage",
  () => {
    it("activates and reuses a complete-identity cache through verifier-compatible paths", async () => {
      const models = await modelsPath();
      const storage = createPosixModelStorageCapability(models);
      expect(
        await storage.withInstalledFile(identity, signal(), () => Promise.resolve(true)),
      ).toEqual({
        kind: "missing",
      });
      const staging = await storage.createStagingFile(identity, "install-1", signal());
      await staging.withFilePath((path) => write(path, "verified model"));
      await expect(staging.withFilePath(verifyRegularModelPath)).resolves.toBe("verified model");
      await staging.activate();
      await staging.activate();
      await staging.cleanup();
      await staging.cleanup();
      const cached = createPosixModelStorageCapability(models);
      await expect(
        cached.withInstalledFile(identity, signal(), verifyRegularModelPath),
      ).resolves.toEqual({ kind: "present", value: "verified model" });
      for (const changed of [
        { ...identity, filename: "different.gguf" },
        { ...identity, maxBytes: 65 },
        { ...identity, sha256: "c".repeat(64) },
      ]) {
        await expect(
          cached.withInstalledFile(changed, signal(), () => Promise.resolve(true)),
        ).resolves.toEqual({ kind: "missing" });
      }
      expect((await readdir(models)).filter((name) => name.startsWith(".staging-"))).toEqual([]);
    });

    it("rejects invalid input and cancellation without creating the models directory", async () => {
      const models = await modelsPath();
      const storage = createPosixModelStorageCapability(models);
      for (const changed of [
        { ...identity, filename: "../escape" },
        { ...identity, revision: "main" },
        { ...identity, maxBytes: 0 },
      ]) {
        await expect(storage.acquireInstallLockAnchor(changed, signal())).rejects.toThrow(
          "native_model_identity_invalid",
        );
      }
      await expect(storage.createStagingFile(identity, "../escape", signal())).rejects.toThrow(
        "native_model_operation_invalid",
      );
      const cancelled = AbortSignal.abort();
      await expect(storage.createStagingFile(identity, "cancelled", cancelled)).rejects.toThrow(
        "model_download_cancelled",
      );
      await expect(
        storage.withInstalledFile(identity, cancelled, () => Promise.resolve(true)),
      ).rejects.toThrow("model_download_cancelled");
      await expect(storage.acquireInstallLockAnchor(identity, cancelled)).rejects.toThrow(
        "model_download_cancelled",
      );
      await expect(readdir(models)).rejects.toMatchObject({ code: "ENOENT" });
    });

    it("rejects symlink ancestors, permissive roots, replaced roots, and symlink model leaves", async () => {
      const models = await modelsPath();
      await mkdir(models, { mode: 0o755 });
      const storage = createPosixModelStorageCapability(models);
      await expect(
        storage.withInstalledFile(identity, signal(), () => Promise.resolve(true)),
      ).rejects.toThrow("native_model_storage_failed");
      await chmod(models, 0o700);
      await storage.withInstalledFile(identity, signal(), () => Promise.resolve(true));
      const outside = join(models, "outside");
      await write(outside, "safe");
      await symlink(outside, join(models, `model-${modelKey(identity)}.gguf`));
      await expect(
        storage.withInstalledFile(identity, signal(), () => Promise.resolve(true)),
      ).rejects.toThrow("native_model_storage_failed");
      await rename(models, `${models}.old`);
      await mkdir(models, { mode: 0o700 });
      await expect(
        storage.withInstalledFile(identity, signal(), () => Promise.resolve(true)),
      ).rejects.toThrow("native_model_storage_failed");
      const throughLink = `${models}.link`;
      await symlink(models, throughLink);
      await expect(
        createPosixModelStorageCapability(join(throughLink, "nested")).acquireInstallLockAnchor(
          identity,
          signal(),
        ),
      ).rejects.toThrow("native_model_storage_failed");
      await expect(readdir(join(models, "nested"))).rejects.toMatchObject({ code: "ENOENT" });
    });

    it("excludes owned activation during installed callbacks across adapter instances", async () => {
      const models = await modelsPath();
      const storage = createPosixModelStorageCapability(models);
      const first = await storage.createStagingFile(identity, "first", signal());
      await first.withFilePath((path) => write(path, "first"));
      await first.activate();
      await first.cleanup();
      const other = createPosixModelStorageCapability(models);
      const next = await other.createStagingFile(identity, "next", signal());
      await next.withFilePath((path) => write(path, "next"));
      await storage.withInstalledFile(identity, signal(), async (path) => {
        await expect(next.activate()).rejects.toThrow("native_model_storage_failed");
        expect(await readFile(path, "utf8")).toBe("first");
        await expect(
          other.withInstalledFile(identity, signal(), (file) => readFile(file, "utf8")),
        ).resolves.toEqual({ kind: "present", value: "first" });
      });
      await next.activate();
      await next.cleanup();
      await expect(
        storage.withInstalledFile(identity, signal(), (path) => readFile(path, "utf8")),
      ).resolves.toEqual({ kind: "present", value: "next" });
    });

    it("protects callback ownership and cleans partial and oversized downloads", async () => {
      const models = await modelsPath();
      const storage = createPosixModelStorageCapability(models);
      const staging = await storage.createStagingFile(identity, "partial", signal());
      const failure = new Error("download stopped");
      await expect(
        staging.withFilePath(async (path) => {
          await write(path, "partial");
          await expect(staging.cleanup()).rejects.toThrow("native_model_storage_failed");
          await expect(staging.activate()).rejects.toThrow("native_model_storage_failed");
          await expect(staging.withFilePath(() => Promise.resolve(true))).rejects.toThrow(
            "native_model_storage_failed",
          );
          throw failure;
        }),
      ).rejects.toBe(failure);
      await staging.cleanup();
      const oversize = await storage.createStagingFile(identity, "oversize", signal());
      await expect(oversize.withFilePath((path) => write(path, Buffer.alloc(65)))).rejects.toThrow(
        "native_model_file_oversize",
      );
      await oversize.cleanup();
      await expect(staging.activate()).rejects.toThrow("native_model_storage_failed");
      expect((await readdir(models)).filter((name) => name.startsWith(".staging-"))).toEqual([]);
    });

    it("detects external inode/content mutation and preserves callback failures", async () => {
      const models = await modelsPath();
      const storage = createPosixModelStorageCapability(models);
      const staging = await storage.createStagingFile(identity, "mutation", signal());
      let original = "";
      await staging.withFilePath(async (path) => {
        original = path;
        await write(path, "original");
      });
      await writeFile(original, "changed");
      await expect(staging.activate()).rejects.toThrow("native_model_storage_failed");
      await staging.cleanup();
      const good = await storage.createStagingFile(identity, "good", signal());
      await good.withFilePath((path) => write(path, "good"));
      await good.activate();
      await good.cleanup();
      const failure = new Error("verifier rejected model");
      await expect(
        storage.withInstalledFile(identity, signal(), () => Promise.reject(failure)),
      ).rejects.toBe(failure);
      const controller = new AbortController();
      await expect(
        storage.withInstalledFile(identity, controller.signal, () => {
          controller.abort();
          return Promise.resolve(true);
        }),
      ).rejects.toThrow("model_download_cancelled");
    });

    it("retains install anchors until callbacks finish and closes idempotently", async () => {
      const storage = createPosixModelStorageCapability(await modelsPath());
      const first = await storage.acquireInstallLockAnchor(identity, signal());
      const second = await storage.acquireInstallLockAnchor(identity, signal());
      const path = await first.withLockTarget(async (target) => {
        await expect(first.release()).rejects.toThrow("native_model_storage_failed");
        expect(await second.withLockTarget((other) => Promise.resolve(other))).toBe(target);
        return target;
      });
      await first.release();
      await first.release();
      await second.release();
      expect(await readFile(path)).toHaveLength(0);
      await expect(first.withLockTarget(() => Promise.resolve(true))).rejects.toThrow(
        "native_model_storage_failed",
      );
    });
  },
);
