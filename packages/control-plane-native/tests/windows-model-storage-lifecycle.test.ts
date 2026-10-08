import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import type { NativeModelIdentity } from "@lyonbrown/local-model/composition";
import { afterEach, describe, expect, it } from "vitest";

import { createNativeModelStorageCapability } from "@lyonbrown/control-plane-native/internal/index";
import {
  createPrivateChildDirectory,
  openModelsDirectory,
} from "@lyonbrown/control-plane-native/internal/windows/model-directory";
import { createPrivateModelFile } from "@lyonbrown/control-plane-native/internal/windows/model-files";
import { identityKey } from "@lyonbrown/control-plane-native/internal/windows/model-identity";

const windows = process.platform === "win32";
const roots: string[] = [];
const identity: NativeModelIdentity = Object.freeze({
  modelId: "qwen3-1.7b-q4_k_m",
  revision: "7fb011e9aee6e4dc7adf8430df9ea8de6a466aa3",
  sha256: "228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f",
  filename: "Qwen3-1.7B-Q4_K_M.gguf",
  maxBytes: 64,
});

async function modelsPath(): Promise<string> {
  const value = await mkdtemp(join(tmpdir(), "lyonbrown-model-lifecycle-"));
  roots.push(value);
  return join(value, "data", "models");
}

afterEach(async () => {
  for (const value of roots.splice(0)) await rm(value, { force: true, recursive: true });
});

describe.runIf(windows)("Windows native model storage lifecycle", () => {
  it("anchors the process lock target until an idempotent release", async () => {
    const storage = createNativeModelStorageCapability(await modelsPath());
    const anchor = await storage.acquireInstallLockAnchor(identity, new AbortController().signal);
    const secondAnchor = await storage.acquireInstallLockAnchor(
      identity,
      new AbortController().signal,
    );
    const target = await anchor.withLockTarget(async (path) => {
      await expect(rename(path, `${path}.moved`)).rejects.toBeDefined();
      await expect(rm(path)).rejects.toBeDefined();
      return path;
    });

    await anchor.release();
    await anchor.release();
    await secondAnchor.release();
    await expect(anchor.withLockTarget(() => Promise.resolve(undefined))).rejects.toThrow(
      "native_model_storage_failed",
    );
    await expect(readFile(target)).resolves.toBeInstanceOf(Buffer);
  });

  it("fails cancelled operations before opening privileged storage", async () => {
    const controller = new AbortController();
    controller.abort();
    const storage = createNativeModelStorageCapability(await modelsPath());

    await expect(storage.acquireInstallLockAnchor(identity, controller.signal)).rejects.toThrow(
      "model_download_cancelled",
    );
    await expect(
      storage.createStagingFile(identity, "operation-cancelled", controller.signal),
    ).rejects.toThrow("model_download_cancelled");
    await expect(
      storage.withInstalledFile(identity, controller.signal, () => Promise.resolve(true)),
    ).rejects.toThrow("model_download_cancelled");
  });

  it("reports a missing installed model without invoking the callback", async () => {
    const storage = createNativeModelStorageCapability(await modelsPath());
    let called = false;

    await expect(
      storage.withInstalledFile(identity, new AbortController().signal, () => {
        called = true;
        return Promise.resolve(true);
      }),
    ).resolves.toEqual({ kind: "missing" });
    expect(called).toBe(false);
  });

  it("rejects oversized staging content and still cleans it", async () => {
    const storage = createNativeModelStorageCapability(await modelsPath());
    const staging = await storage.createStagingFile(
      identity,
      "operation-oversize",
      new AbortController().signal,
    );

    await expect(
      staging.withFilePath((filePath) => writeFile(filePath, Buffer.alloc(identity.maxBytes + 1))),
    ).rejects.toThrow("native_model_file_oversize");
    await staging.cleanup();
    await staging.cleanup();
    await expect(staging.activate()).rejects.toThrow("native_model_storage_failed");
    await expect(staging.withFilePath(() => Promise.resolve(undefined))).rejects.toThrow(
      "native_model_storage_failed",
    );
  });

  it("cleans an unused writable staging file", async () => {
    const storage = createNativeModelStorageCapability(await modelsPath());
    const staging = await storage.createStagingFile(
      identity,
      "operation-unused",
      new AbortController().signal,
    );

    await staging.cleanup();
    await staging.cleanup();
  });

  it("seals a staging callback against reentrancy and activation", async () => {
    const storage = createNativeModelStorageCapability(await modelsPath());
    const staging = await storage.createStagingFile(
      identity,
      "operation-reentrant",
      new AbortController().signal,
    );

    await staging.withFilePath(async (filePath) => {
      await writeFile(filePath, "model", "utf8");
      await expect(staging.withFilePath(() => Promise.resolve(undefined))).rejects.toThrow(
        "native_model_storage_failed",
      );
      await expect(staging.activate()).rejects.toThrow("native_model_storage_failed");
    });
    await staging.activate();
    await staging.activate();
    await expect(staging.withFilePath(() => Promise.resolve(undefined))).rejects.toThrow(
      "native_model_storage_failed",
    );
    await staging.cleanup();
  });

  it("activates an empty staging file without exposing a write callback", async () => {
    const storage = createNativeModelStorageCapability(await modelsPath());
    const staging = await storage.createStagingFile(
      identity,
      "operation-empty",
      new AbortController().signal,
    );

    await staging.activate();
    await staging.cleanup();
    await expect(
      storage.withInstalledFile(identity, new AbortController().signal, (path) => readFile(path)),
    ).resolves.toEqual({ kind: "present", value: Buffer.alloc(0) });

    const callbackFailure = new Error("model verifier rejected content");
    await expect(
      storage.withInstalledFile(identity, new AbortController().signal, () =>
        Promise.reject(callbackFailure),
      ),
    ).rejects.toBe(callbackFailure);

    const controller = new AbortController();
    await expect(
      storage.withInstalledFile(identity, controller.signal, () => {
        controller.abort();
        return Promise.resolve(true);
      }),
    ).rejects.toThrow("model_download_cancelled");
  });

  it("removes a verified stale staging directory after a successful activation", async () => {
    const models = await modelsPath();
    const storage = createNativeModelStorageCapability(models);
    await storage.withInstalledFile(identity, new AbortController().signal, () =>
      Promise.resolve(undefined),
    );
    const staleDirectory = join(models, `.staging-${identityKey(identity)}-${"a".repeat(32)}`);
    const directory = openModelsDirectory(models);
    const staleDirectoryHandle = createPrivateChildDirectory(directory, staleDirectory);
    const staleFileHandle = createPrivateModelFile(join(staleDirectory, identity.filename));
    staleFileHandle.close();
    staleDirectoryHandle.close();
    directory.close();
    await writeFile(join(staleDirectory, identity.filename), "abandoned");
    const staging = await storage.createStagingFile(
      identity,
      "operation-after-crash",
      new AbortController().signal,
    );

    await staging.withFilePath((filePath) => writeFile(filePath, "replacement"));
    await staging.activate();
    try {
      await expect(
        readFile(join(models, `model-${identityKey(identity)}.gguf`), "utf8"),
      ).resolves.toBe("replacement");
      await expect(rm(staleDirectory)).rejects.toMatchObject({ code: "ENOENT" });
    } finally {
      await staging.cleanup();
    }
  });

  it("detects models directory replacement across privileged operations", async () => {
    const models = await modelsPath();
    const storage = createNativeModelStorageCapability(models);
    await expect(
      storage.withInstalledFile(identity, new AbortController().signal, () =>
        Promise.resolve(true),
      ),
    ).resolves.toEqual({ kind: "missing" });
    await rename(models, `${models}.old`);
    await mkdir(dirname(models), { recursive: true });
    await mkdir(models);

    await expect(
      storage.withInstalledFile(identity, new AbortController().signal, () =>
        Promise.resolve(true),
      ),
    ).rejects.toThrow("native_model_storage_failed");
  });

  it("validates operation identifiers before creating staging state", async () => {
    const storage = createNativeModelStorageCapability(await modelsPath());
    for (const operationId of ["", "../escape", "bad\\name", "x".repeat(129)]) {
      await expect(
        storage.createStagingFile(identity, operationId, new AbortController().signal),
      ).rejects.toThrow("native_model_operation_invalid");
    }
  });
});
