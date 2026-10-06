import { mkdir, mkdtemp, readFile, rm, utimes, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_PRESET } from "../src/internal/catalog.ts";
import type { ModelFileDownloadRequest } from "../src/internal/model-download.ts";
import { NodeModelIo } from "../src/internal/node-model-io.ts";
import { TestNativeModelStorage } from "./support/native-storage.ts";

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  );
});

describe("model I/O adapter", () => {
  it("serializes installers across independent adapters with a cancellable process lock", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    const firstIo = new NodeModelIo(new TestNativeModelStorage(root));
    const secondIo = new NodeModelIo(new TestNativeModelStorage(root));
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
    const firstIo = new NodeModelIo(new TestNativeModelStorage(root));
    const secondIo = new NodeModelIo(new TestNativeModelStorage(root));
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

  it("recovers a stale process lock", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    const target = join(root, `.${DEFAULT_PRESET.id}.install`);
    await writeFile(target, "");
    const staleDirectory = `${target}.lock`;
    await mkdir(staleDirectory);
    const staleTime = new Date(Date.now() - 60_000);
    await utimes(staleDirectory, staleTime, staleTime);
    const io = new NodeModelIo(new TestNativeModelStorage(root));

    const lease = await io.acquireInstallLock(DEFAULT_PRESET, new AbortController().signal);

    await lease.release();
  });

  it.each([
    { downloadedBytes: -1, totalBytes: 10 },
    { downloadedBytes: Number.NaN, totalBytes: 10 },
    { downloadedBytes: 11, totalBytes: 10 },
    { downloadedBytes: 1025, totalBytes: 1025 },
    { downloadedBytes: 1, totalBytes: Number.MAX_SAFE_INTEGER + 1 },
  ])("actively aborts invalid downloader progress %#", async (invalidProgress) => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    let continuedAfterInvalidProgress = false;
    const io = new NodeModelIo(new TestNativeModelStorage(root), {
      downloadModelFile: (request) => {
        request.onProgress(invalidProgress);
        continuedAfterInvalidProgress = !request.signal.aborted;
        return request.signal.aborted
          ? Promise.reject(new Error("aborted at I/O boundary"))
          : Promise.resolve();
      },
    });
    const preset = { ...DEFAULT_PRESET, maxBytes: 1024 } as const;

    await expect(
      io.download(preset, {
        operationId: "op-invalid-progress",
        signal: new AbortController().signal,
        onProgress: vi.fn(),
      }),
    ).rejects.toThrow();
    expect(continuedAfterInvalidProgress).toBe(false);
  });

  it("aborts when downloader progress moves backwards", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    let abortedBeforeContinuing = false;
    const io = new NodeModelIo(new TestNativeModelStorage(root), {
      downloadModelFile: (request) => {
        request.onProgress({ downloadedBytes: 8, totalBytes: 10 });
        request.onProgress({ downloadedBytes: 7, totalBytes: 10 });
        abortedBeforeContinuing = request.signal.aborted;
        return Promise.reject(new Error("stopped"));
      },
    });

    await expect(
      io.download(DEFAULT_PRESET, {
        operationId: "op-regression",
        signal: new AbortController().signal,
        onProgress: vi.fn(),
      }),
    ).rejects.toThrow();
    expect(abortedBeforeContinuing).toBe(true);
  });

  it("aborts when downloader total bytes move backwards", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    let abortedBeforeContinuing = false;
    const io = new NodeModelIo(new TestNativeModelStorage(root), {
      downloadModelFile: (request) => {
        request.onProgress({ downloadedBytes: 7, totalBytes: 10 });
        request.onProgress({ downloadedBytes: 7, totalBytes: 9 });
        abortedBeforeContinuing = request.signal.aborted;
        return Promise.reject(new Error("stopped"));
      },
    });

    await expect(
      io.download(DEFAULT_PRESET, {
        operationId: "op-total-regression",
        signal: new AbortController().signal,
        onProgress: vi.fn(),
      }),
    ).rejects.toThrow();
    expect(abortedBeforeContinuing).toBe(true);
  });

  it("uses only the pinned source and activates a verified staged fixture", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    const content = Buffer.from("fixture");
    const downloadModelFile = vi.fn(async (request: ModelFileDownloadRequest) => {
      if (request.signal.aborted) throw new Error("aborted");
      await writeFile(request.targetPath, content);
    });
    const storage = new TestNativeModelStorage(root);
    const io = new NodeModelIo(storage, { downloadModelFile });
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
    expect(downloadModelFile).toHaveBeenCalledOnce();
    const request = downloadModelFile.mock.calls[0]?.[0];
    expect(request?.source).toBe(
      "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/7fb011e9aee6e4dc7adf8430df9ea8de6a466aa3/Qwen3-1.7B-Q4_K_M.gguf",
    );
    expect(request?.targetPath).toContain(".staging-");
    expect(request?.maxBytes).toBe(1024);
    expect(request?.signal.aborted).toBe(false);
    await expect(io.verify(staged, preset, controller.signal)).resolves.toBe(content.byteLength);
    await io.activate(staged);
    await expect(io.inspectInstalled(preset, controller.signal)).resolves.toBe(true);
    await expect(readFile(join(root, preset.filename))).resolves.toEqual(content);
  });

  it("rejects a wrong digest and never treats it as installed", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    const downloadModelFile = vi.fn(async (request: ModelFileDownloadRequest) => {
      await writeFile(request.targetPath, "corrupt");
    });
    const io = new NodeModelIo(new TestNativeModelStorage(root), { downloadModelFile });
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

  it("removes staging data when the streaming download fails", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-local-model-"));
    temporaryDirectories.push(root);
    const io = new NodeModelIo(new TestNativeModelStorage(root), {
      downloadModelFile: () => Promise.reject(new Error("network failed")),
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
