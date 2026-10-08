import { mkdtemp, readFile, readdir, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  createNativeModelBundleStorageCapability,
  type NativeModelBundleIdentity,
} from "@lyonbrown/control-plane-native/internal/index";
import {
  bundleIndexName,
  modelBundleKey,
} from "@lyonbrown/control-plane-native/internal/windows/model-bundle-identity";
import { afterEach, describe, expect, it } from "vitest";

const windows = process.platform === "win32";
const roots: string[] = [];
const configurationFile = Object.freeze({
  path: "config.json",
  sha256: "36450d7fcfa7ab7708b597d0096192acd8a7ee4a000000000000000000000000",
  maxBytes: 4_096,
});
const weightsFile = Object.freeze({
  path: "onnx/model_q4.onnx",
  sha256: "db4657bb2f14f073cb4d289260dfc50af31adc1260a04cb2fe17c4f03da0881a",
  maxBytes: 2_000_000,
});
const identity: NativeModelBundleIdentity = Object.freeze({
  modelId: "kev-0.6b",
  revision: "5a827091b3d58fe4fdfc02a5a5b2ce364047d678",
  files: Object.freeze([configurationFile, weightsFile]),
});

async function root(): Promise<string> {
  const value = await mkdtemp(join(tmpdir(), "lyonbrown-model-bundle-"));
  roots.push(value);
  return value;
}

afterEach(async () => {
  for (const value of roots.splice(0)) await rm(value, { force: true, recursive: true });
});

describe.runIf(windows)("Windows native model bundle storage", () => {
  it("atomically installs and anchors an identity-bound model directory", async () => {
    const models = join(await root(), "data", "decision-models");
    const storage = createNativeModelBundleStorageCapability(models);
    const staging = await storage.createStagingBundle(
      identity,
      "bundle-operation-1",
      new AbortController().signal,
    );

    await staging.withDirectoryPath(async (directoryPath) => {
      await writeFile(join(directoryPath, "config.json"), "configuration", "utf8");
      await writeFile(join(directoryPath, "onnx", "model_q4.onnx"), "weights", "utf8");
    });
    await staging.activate();
    await staging.cleanup();

    const installed = await storage.withInstalledBundle(
      identity,
      new AbortController().signal,
      async (directoryPath) => {
        await expect(rename(directoryPath, `${directoryPath}.moved`)).rejects.toBeDefined();
        return Promise.all([
          readFile(join(directoryPath, "config.json"), "utf8"),
          readFile(join(directoryPath, "onnx", "model_q4.onnx"), "utf8"),
        ]);
      },
    );

    expect(installed).toEqual({ kind: "present", value: ["configuration", "weights"] });
  });

  it("rejects traversal, duplicate paths, and invalid bundle metadata before filesystem access", async () => {
    const storage = createNativeModelBundleStorageCapability(
      join(await root(), "data", "decision-models"),
    );
    const malformed: readonly NativeModelBundleIdentity[] = [
      { ...identity, files: [{ ...configurationFile, path: "../escape" }] },
      { ...identity, files: [{ ...configurationFile, path: "C:\\escape" }] },
      { ...identity, files: [configurationFile, configurationFile] },
      { ...identity, files: [] },
      { ...identity, files: null } as unknown as NativeModelBundleIdentity,
      { ...identity, files: Array.from({ length: 33 }, () => configurationFile) },
      { ...identity, revision: "main" },
      { ...identity, revision: "A".repeat(40) },
      { ...identity, modelId: "../kev" },
      { ...identity, modelId: "KEV" },
      { ...identity, files: [{ ...configurationFile, sha256: "invalid" }] },
      { ...identity, files: [{ ...configurationFile, maxBytes: 0 }] },
      { ...identity, files: [{ ...configurationFile, path: "nested/" }] },
      { ...identity, files: [{ ...configurationFile, path: "nested//file" }] },
      { ...identity, files: [{ ...configurationFile, path: "con" }] },
      { ...identity, files: [{ ...configurationFile, path: "file." }] },
      { ...identity, files: [{ ...configurationFile, path: String.fromCharCode(0xd800) }] },
      { ...identity, files: [{ ...configurationFile, path: "a".repeat(257) }] },
    ];

    expect(() =>
      modelBundleKey({ ...identity, files: [{ ...configurationFile, path: "model-😀.bin" }] }),
    ).not.toThrow();

    for (const candidate of malformed) {
      await expect(
        storage.createStagingBundle(
          candidate,
          "bundle-operation-invalid",
          new AbortController().signal,
        ),
      ).rejects.toThrow("native_model_bundle_identity_invalid");
    }

    await expect(
      storage.createStagingBundle(identity, "../invalid", new AbortController().signal),
    ).rejects.toThrow("native_model_operation_invalid");
    const aborted = new AbortController();
    aborted.abort();
    await expect(
      storage.createStagingBundle(identity, "bundle-operation-aborted", aborted.signal),
    ).rejects.toThrow("model_download_cancelled");
  });

  it("seals every staged file after the writer callback and removes partial bundles", async () => {
    const models = join(await root(), "data", "decision-models");
    const storage = createNativeModelBundleStorageCapability(models);
    const staging = await storage.createStagingBundle(
      identity,
      "bundle-operation-seal",
      new AbortController().signal,
    );
    let configPath = "";
    await staging.withDirectoryPath(async (directoryPath) => {
      configPath = join(directoryPath, "config.json");
      await writeFile(configPath, "configuration", "utf8");
      await writeFile(join(directoryPath, "onnx", "model_q4.onnx"), "weights", "utf8");
    });

    await expect(writeFile(configPath, "late mutation", "utf8")).rejects.toBeDefined();
    await staging.cleanup();
    await staging.cleanup();
    await expect(readdir(models)).resolves.toEqual([]);
  });

  it("rejects overlapping staging callbacks and cleanup while a writer is active", async () => {
    const models = join(await root(), "data", "decision-models");
    const storage = createNativeModelBundleStorageCapability(models);
    const staging = await storage.createStagingBundle(
      identity,
      "bundle-operation-overlap",
      new AbortController().signal,
    );
    let release: (() => void) | undefined;
    const active = staging.withDirectoryPath(async (directoryPath) => {
      await writeFile(join(directoryPath, "config.json"), "configuration", "utf8");
      await writeFile(join(directoryPath, "onnx", "model_q4.onnx"), "weights", "utf8");
      await new Promise<void>((resolve) => {
        release = resolve;
      });
    });
    await expect.poll(() => release).toBeTypeOf("function");

    await expect(staging.withDirectoryPath(() => Promise.resolve(undefined))).rejects.toThrow(
      "native_model_storage_failed",
    );
    await expect(staging.cleanup()).rejects.toThrow("native_model_storage_failed");
    release?.();
    await active;
    await staging.cleanup();
  });

  it("supports idempotent activation and revalidates sealed files", async () => {
    const models = join(await root(), "data", "decision-models");
    const storage = createNativeModelBundleStorageCapability(models);
    const staging = await storage.createStagingBundle(
      identity,
      "bundle-operation-idempotent",
      new AbortController().signal,
    );
    await staging.withDirectoryPath(async (directoryPath) => {
      await writeFile(join(directoryPath, "config.json"), "configuration", "utf8");
      await writeFile(join(directoryPath, "onnx", "model_q4.onnx"), "weights", "utf8");
    });
    await staging.withDirectoryPath(async (directoryPath) => {
      await expect(readFile(join(directoryPath, "config.json"), "utf8")).resolves.toBe(
        "configuration",
      );
    });
    await staging.activate();
    await staging.activate();
    await staging.cleanup();
  });

  it("prevents a sealed artifact from being renamed and replaced before activation", async () => {
    const models = join(await root(), "data", "decision-models");
    const storage = createNativeModelBundleStorageCapability(models);
    const staging = await storage.createStagingBundle(
      identity,
      "bundle-operation-replacement-race",
      new AbortController().signal,
    );
    let configurationPath = "";
    await staging.withDirectoryPath(async (directoryPath) => {
      configurationPath = join(directoryPath, "config.json");
      await writeFile(configurationPath, "configuration", "utf8");
      await writeFile(join(directoryPath, "onnx", "model_q4.onnx"), "weights", "utf8");
    });

    try {
      await expect(
        rename(configurationPath, `${configurationPath}.replaced`),
      ).rejects.toBeDefined();
      await expect(
        writeFile(configurationPath, "unverified replacement", "utf8"),
      ).rejects.toBeDefined();
      await staging.activate();
    } finally {
      await staging.cleanup();
    }
  });

  it("reports a missing bundle and rejects a corrupted activation index", async () => {
    const models = join(await root(), "data", "decision-models");
    const storage = createNativeModelBundleStorageCapability(models);
    await expect(
      storage.withInstalledBundle(identity, new AbortController().signal, () =>
        Promise.resolve("unused"),
      ),
    ).resolves.toEqual({ kind: "missing" });
    const staging = await storage.createStagingBundle(
      identity,
      "bundle-operation-corrupt-index",
      new AbortController().signal,
    );
    await staging.withDirectoryPath(async (directoryPath) => {
      await writeFile(join(directoryPath, "config.json"), "configuration", "utf8");
      await writeFile(join(directoryPath, "onnx", "model_q4.onnx"), "weights", "utf8");
    });
    await staging.activate();
    await staging.cleanup();
    await writeFile(join(models, bundleIndexName(identity)), "{}", "utf8");

    await expect(
      storage.withInstalledBundle(identity, new AbortController().signal, () =>
        Promise.resolve("unused"),
      ),
    ).rejects.toThrow("native_model_storage_failed");
  });

  it("removes a partial bundle when sealing discovers a missing file", async () => {
    const models = join(await root(), "data", "decision-models");
    const storage = createNativeModelBundleStorageCapability(models);
    const staging = await storage.createStagingBundle(
      identity,
      "bundle-operation-partial",
      new AbortController().signal,
    );
    await expect(
      staging.withDirectoryPath(async (directoryPath) => {
        await writeFile(join(directoryPath, "config.json"), "configuration", "utf8");
      }),
    ).rejects.toThrow("native_model_storage_failed");
    await staging.cleanup();
    await expect(readdir(models)).resolves.toEqual([]);
  });
});
