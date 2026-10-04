import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_PRESET } from "../src/internal/catalog.js";
import { createNodeLlamaEngine } from "../src/internal/node-llama.js";
import { NodeModelIo } from "../src/internal/node-model-io.js";

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  );
});

describe("node-llama-cpp production boundary", () => {
  it("loads only official prebuilds and forbids source downloads and compilation fallback", async () => {
    const dispose = vi.fn(() => Promise.resolve());
    const getLlama = vi.fn(() =>
      Promise.resolve({
        loadModel: vi.fn(() => Promise.resolve({ dispose })),
      }),
    );
    const engine = createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", { getLlama });

    const lease = await engine.loadDefault();

    expect(getLlama).toHaveBeenCalledWith({
      build: "never",
      skipDownload: true,
      usePrebuiltBinaries: true,
      progressLogs: false,
    });
    expect(getLlama.mock.results[0]?.value).toBeInstanceOf(Promise);
    await lease.dispose();
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("disposes the binding when model loading fails and redacts dispose failures", async () => {
    const disposeLlama = vi.fn(() => Promise.resolve());
    const engine = createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", {
      getLlama: () =>
        Promise.resolve({
          loadModel: () => Promise.reject(new Error("native path leak")),
          dispose: disposeLlama,
        }),
    });
    await expect(engine.loadDefault()).rejects.toThrow("native path leak");
    expect(disposeLlama).toHaveBeenCalledOnce();

    const failingLease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", {
      getLlama: () =>
        Promise.resolve({
          loadModel: () =>
            Promise.resolve({
              dispose: () => Promise.reject(new Error("C:\\private\\native model")),
            }),
          dispose: disposeLlama,
        }),
    }).loadDefault();
    await expect(failingLease.dispose()).rejects.toThrow("native_runtime_dispose_failed");
  });
});

describe("model I/O adapter", () => {
  it("serializes installers across independent adapters with a cancellable process lock", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    const firstIo = new NodeModelIo(root, { createModelDownloader: vi.fn() });
    const secondIo = new NodeModelIo(root, { createModelDownloader: vi.fn() });
    const first = await firstIo.acquireInstallLock(DEFAULT_PRESET, new AbortController().signal);
    let secondAcquired = false;
    const secondPromise = secondIo
      .acquireInstallLock(DEFAULT_PRESET, new AbortController().signal)
      .then((lock) => {
        secondAcquired = true;
        return lock;
      });
    await new Promise<void>((resolve) => setTimeout(resolve, 75));
    expect(secondAcquired).toBe(false);

    await first.release();
    const second = await secondPromise;
    expect(secondAcquired).toBe(true);
    await second.release();
    await second.release();
  });

  it("cancels a process-lock waiter without taking the active lock", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    const firstIo = new NodeModelIo(root, { createModelDownloader: vi.fn() });
    const secondIo = new NodeModelIo(root, { createModelDownloader: vi.fn() });
    const first = await firstIo.acquireInstallLock(DEFAULT_PRESET, new AbortController().signal);
    const controller = new AbortController();
    const waiting = secondIo.acquireInstallLock(DEFAULT_PRESET, controller.signal);
    controller.abort();

    await expect(waiting).rejects.toThrow("model_download_cancelled");
    await first.release();

    const alreadyCancelled = new AbortController();
    alreadyCancelled.abort();
    await expect(
      firstIo.acquireInstallLock(DEFAULT_PRESET, alreadyCancelled.signal),
    ).rejects.toThrow("model_download_cancelled");
  });

  it("uses only the pinned source and activates a verified staged fixture", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    const content = Buffer.from("fixture");
    const createModelDownloader = vi.fn((options: { dirPath: string; fileName: string }) =>
      Promise.resolve({
        download: async ({ signal }: { signal?: AbortSignal } = {}) => {
          if (signal?.aborted) throw new Error("aborted");
          const path = join(options.dirPath, options.fileName);
          await writeFile(path, content);
          return path;
        },
      }),
    );
    const io = new NodeModelIo(root, { createModelDownloader });
    const preset = {
      ...DEFAULT_PRESET,
      sha256: "f16d05ec6b29248d2c61adb1e9263f78e4f7bace1b955014a2d17872cfe4064d",
      expectedDownloadBytes: content.byteLength,
      maxBytes: 1024,
    } as const;
    const controller = new AbortController();
    await writeFile(join(root, preset.filename), "previous-corrupt-cache");

    const staged = await io.download(preset, {
      operationId: "op-1",
      signal: controller.signal,
      onProgress: vi.fn(),
    });
    expect(createModelDownloader).toHaveBeenCalledWith(
      expect.objectContaining({
        modelUri:
          "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/7fb011e9aee6e4dc7adf8430df9ea8de6a466aa3/Qwen3-1.7B-Q4_K_M.gguf",
        fileName: "Qwen3-1.7B-Q4_K_M.gguf",
        showCliProgress: false,
        skipExisting: false,
        deleteTempFileOnCancel: true,
      }),
    );
    await expect(io.verify(staged, preset, controller.signal)).resolves.toBe(content.byteLength);
    await io.activate(staged, preset);
    await expect(io.inspectInstalled(preset, controller.signal)).resolves.toBe(true);
    await expect(readFile(join(root, preset.filename))).resolves.toEqual(content);
  });

  it("rejects a wrong digest and never treats it as installed", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    const createModelDownloader = vi.fn((options: { dirPath: string; fileName: string }) =>
      Promise.resolve({
        download: async () => {
          const path = join(options.dirPath, options.fileName);
          await writeFile(path, "corrupt");
          return path;
        },
      }),
    );
    const io = new NodeModelIo(root, { createModelDownloader });
    const controller = new AbortController();
    const staged = await io.download(DEFAULT_PRESET, {
      operationId: "op-1",
      signal: controller.signal,
      onProgress: vi.fn(),
    });

    await expect(io.verify(staged, DEFAULT_PRESET, controller.signal)).rejects.toThrow(
      "model_integrity_failed",
    );
    await io.cleanup(staged);
    await expect(io.inspectInstalled(DEFAULT_PRESET, controller.signal)).resolves.toBe(false);
  });

  it("removes staging data when the official downloader fails before returning an artifact", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    const io = new NodeModelIo(root, {
      createModelDownloader: () =>
        Promise.resolve({
          download: () => Promise.reject(new Error("network failed")),
        }),
    });

    await expect(
      io.download(DEFAULT_PRESET, {
        operationId: "op-1",
        signal: new AbortController().signal,
        onProgress: vi.fn(),
      }),
    ).rejects.toThrow("network failed");

    await expect(readFile(join(root, DEFAULT_PRESET.filename))).rejects.toMatchObject({
      code: "ENOENT",
    });
    const { readdir } = await import("node:fs/promises");
    await expect(readdir(root)).resolves.toEqual([]);
  });
});
