import { mkdtemp, readFile, readdir, rename, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { NativeModelIdentity } from "@lyonbrown/local-model/composition";
import { afterEach, describe, expect, it } from "vitest";

import { createNativeModelStorageCapability } from "@lyonbrown/control-plane-native/internal/index";

const windows = process.platform === "win32";
const roots: string[] = [];
const identity: NativeModelIdentity = Object.freeze({
  modelId: "qwen3-1.7b-q4_k_m",
  revision: "7fb011e9aee6e4dc7adf8430df9ea8de6a466aa3",
  sha256: "228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f",
  filename: "Qwen3-1.7B-Q4_K_M.gguf",
  maxBytes: 2_000_000_000,
});

async function root(): Promise<string> {
  const value = await mkdtemp(join(tmpdir(), "lyonbrown-model-storage-"));
  roots.push(value);
  return value;
}

afterEach(async () => {
  for (const value of roots.splice(0)) await rm(value, { force: true, recursive: true });
});

describe.runIf(windows)("Windows native model storage", () => {
  it("installs and reads a private identity-bound model", async () => {
    const models = join(await root(), "data", "scope", "models");
    const storage = createNativeModelStorageCapability(models);
    const staging = await storage.createStagingFile(
      identity,
      "operation-1",
      new AbortController().signal,
    );
    let exposedPath = "";
    await staging.withFilePath(async (filePath, directoryPath) => {
      exposedPath = filePath;
      expect(filePath).toBe(join(directoryPath, identity.filename));
      await writeFile(filePath, "verified model", "utf8");
    });
    await expect(staging.withFilePath((filePath) => readFile(filePath, "utf8"))).resolves.toBe(
      "verified model",
    );
    await staging.activate();
    await staging.cleanup();

    const result = await storage.withInstalledFile(
      identity,
      new AbortController().signal,
      (filePath) => readFile(filePath, "utf8"),
    );

    expect(result).toEqual({ kind: "present", value: "verified model" });
    expect(exposedPath).not.toContain(identity.sha256);
  });

  it("rejects malformed identities before touching the filesystem", async () => {
    const models = join(await root(), "models");
    const storage = createNativeModelStorageCapability(models);
    const malformed = [
      { ...identity, filename: "..\\escape.gguf" },
      { ...identity, filename: "CON" },
      { ...identity, sha256: "not-a-digest" },
      { ...identity, revision: "main" },
      { ...identity, maxBytes: 0 },
      { ...identity, modelId: "../model" },
      { ...identity, filename: "bad?.gguf" },
      { ...identity, filename: "bad\u0001.gguf" },
      { ...identity, filename: "" },
      { ...identity, filename: "." },
      { ...identity, filename: "bad.gguf " },
      { ...identity, filename: `bad\ud800.gguf` },
      { ...identity, filename: `bad\udc00.gguf` },
      { ...identity, sha256: identity.sha256.toUpperCase() },
      { ...identity, revision: identity.revision.toUpperCase() },
      { ...identity, maxBytes: Number.MAX_SAFE_INTEGER + 1 },
      { ...identity, modelId: "UPPERCASE" },
    ];
    for (const value of malformed) {
      await expect(
        storage.acquireInstallLockAnchor(value, new AbortController().signal),
      ).rejects.toThrow("native_model_identity_invalid");
    }
  });

  it("rejects unsafe absolute directory syntax before touching the filesystem", () => {
    for (const path of [
      "relative\\models",
      "C:\\safe\\bad:name\\models",
      "C:\\safe\\trailing.\\models",
      "C:\\safe\\AUX\\models",
      "\\\\?\\C:\\safe\\models",
      "C:\\models",
      "C:\\safe\\.\\models",
      "C:\\safe\\..\\models",
      "C:\\safe\\trailing \\models",
      `C:\\safe\\bad\u0001\\models`,
      `C:\\safe\\bad\ud800\\models`,
      `C:\\safe\\bad\udc00\\models`,
      `C:\\safe\u0000models`,
    ]) {
      expect(() => createNativeModelStorageCapability(path)).toThrow(
        "native_model_directory_invalid",
      );
    }
  });

  it("accepts well-formed Unicode path and filename components", async () => {
    const unicodeIdentity = { ...identity, filename: "模型😀.gguf" };
    const storage = createNativeModelStorageCapability(join(await root(), "数据😀", "models"));
    const anchor = await storage.acquireInstallLockAnchor(
      unicodeIdentity,
      new AbortController().signal,
    );
    await anchor.release();
  });

  it("seals staging against rename, delete, and late writes after the download callback", async () => {
    const storage = createNativeModelStorageCapability(join(await root(), "data", "models"));
    const staging = await storage.createStagingFile(
      identity,
      "operation-2",
      new AbortController().signal,
    );
    let filePath = "";
    let directoryPath = "";
    await staging.withFilePath(async (file, directory) => {
      filePath = file;
      directoryPath = directory;
      await writeFile(file, "sealed", "utf8");
    });

    await expect(writeFile(filePath, "late", "utf8")).rejects.toBeDefined();
    await expect(rename(filePath, `${filePath}.moved`)).rejects.toBeDefined();
    await expect(rm(filePath)).rejects.toBeDefined();
    await expect(rename(directoryPath, `${directoryPath}.moved`)).rejects.toBeDefined();
    await staging.cleanup();
    await staging.cleanup();
  });

  it("removes a partial staged file when the download callback fails", async () => {
    const models = join(await root(), "data", "models");
    const storage = createNativeModelStorageCapability(models);
    const staging = await storage.createStagingFile(
      identity,
      "operation-stream-failure",
      new AbortController().signal,
    );

    await expect(
      staging.withFilePath(async (filePath) => {
        await writeFile(filePath, "partial model", "utf8");
        throw new Error("download failed");
      }),
    ).rejects.toThrow("download failed");
    await staging.cleanup();

    await expect(readdir(models)).resolves.toEqual([]);
  });

  it("refuses cleanup while a staging callback is active", async () => {
    const storage = createNativeModelStorageCapability(join(await root(), "data", "models"));
    const staging = await storage.createStagingFile(
      identity,
      "operation-cleanup-race",
      new AbortController().signal,
    );
    let releaseCallback: (() => void) | undefined;
    const callbackStarted = new Promise<void>((resolveStarted) => {
      void staging.withFilePath(async (filePath) => {
        await writeFile(filePath, "complete", "utf8");
        resolveStarted();
        await new Promise<void>((resolve) => {
          releaseCallback = resolve;
        });
      });
    });
    await callbackStarted;

    await expect(staging.cleanup()).rejects.toThrow("native_model_storage_failed");
    releaseCallback?.();
    await new Promise<void>((resolve) => setImmediate(resolve));
    await staging.cleanup();
  });

  it("atomically replaces a corrupt installed file and reanchors the destination", async () => {
    const storage = createNativeModelStorageCapability(join(await root(), "data", "models"));
    const first = await storage.createStagingFile(
      identity,
      "operation-3",
      new AbortController().signal,
    );
    await first.withFilePath((filePath) => writeFile(filePath, "corrupt", "utf8"));
    await first.activate();
    await first.cleanup();

    const second = await storage.createStagingFile(
      identity,
      "operation-4",
      new AbortController().signal,
    );
    await second.withFilePath((filePath) => writeFile(filePath, "replacement", "utf8"));
    await second.activate();
    const installed = await storage.withInstalledFile(
      identity,
      new AbortController().signal,
      async (filePath) => {
        await expect(rename(filePath, `${filePath}.moved`)).rejects.toBeDefined();
        await expect(rm(filePath)).rejects.toBeDefined();
        return readFile(filePath, "utf8");
      },
    );
    expect(installed).toEqual({ kind: "present", value: "replacement" });
    await second.cleanup();
  });

  it("rejects an intermediate junction and never creates through it", async () => {
    const value = await root();
    const outside = join(value, "outside");
    const junction = join(value, "junction");
    await symlink(outside, junction, "junction").catch(async () => {
      await import("node:fs/promises").then(({ mkdir }) => mkdir(outside));
      await symlink(outside, junction, "junction");
    });
    const storage = createNativeModelStorageCapability(join(junction, "models"));
    await expect(
      storage.acquireInstallLockAnchor(identity, new AbortController().signal),
    ).rejects.toThrow("native_model_storage_failed");
  });
});
