import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_PRESET } from "@lyonbrown/local-model/internal/internal/catalog";
import { downloadModelFile } from "@lyonbrown/local-model/internal/internal/model-download";
import { TestNativeModelStorage } from "@lyonbrown/local-model/test/support/native-storage";

const temporaryDirectories: string[] = [];

afterEach(async () => {
  vi.unstubAllGlobals();
  await Promise.all(
    temporaryDirectories.splice(0).map((path) => rm(path, { recursive: true, force: true })),
  );
});

describe("streaming model download", () => {
  it("is the default NodeModelIo download boundary", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-model-download-"));
    temporaryDirectories.push(root);
    const content = Buffer.from("fixture");
    const progress = vi.fn();
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve(responseFromChunks([content], content.byteLength.toString()))),
    );
    const { NodeModelIo } = await import("@lyonbrown/local-model/internal/internal/node-model-io");
    const io = new NodeModelIo(new TestNativeModelStorage(root));
    const preset = {
      ...DEFAULT_PRESET,
      sha256: "f16d05ec6b29248d2c61adb1e9263f78e4f7bace1b955014a2d17872cfe4064d",
      expectedDownloadBytes: content.byteLength,
      maxBytes: 1024,
    } as const;

    const artifact = await io.download(preset, {
      operationId: "op-fetch-default",
      signal: new AbortController().signal,
      onProgress: progress,
    });

    await expect(io.verify(artifact, preset, new AbortController().signal)).resolves.toBe(
      content.byteLength,
    );
    expect(progress).toHaveBeenCalledWith({
      downloadedBytes: content.byteLength,
      totalBytes: content.byteLength,
    });
  });

  it("streams the response to the staging path with strictly increasing progress", async () => {
    const targetPath = await createTargetPath();
    const signal = new AbortController().signal;
    const progress: { downloadedBytes: number; totalBytes: number }[] = [];
    let receivedSignal: AbortSignal | null | undefined;
    const fetcher: typeof globalThis.fetch = (_input, init) => {
      receivedSignal = init?.signal;
      return Promise.resolve(
        responseFromChunks([Uint8Array.from([1, 2]), Uint8Array.from([3, 4, 5])], "5"),
      );
    };

    await downloadModelFile({
      source: "https://example.test/model.gguf",
      targetPath,
      maxBytes: 8,
      signal,
      onProgress: (value) => progress.push(value),
      fetcher,
    });

    expect(receivedSignal).toBe(signal);
    await expect(readFile(targetPath)).resolves.toEqual(Buffer.from([1, 2, 3, 4, 5]));
    expect(progress).toEqual([
      { downloadedBytes: 2, totalBytes: 5 },
      { downloadedBytes: 5, totalBytes: 5 },
    ]);
  });

  it("rejects non-success responses and cancels their bodies", async () => {
    const targetPath = await createTargetPath();
    const bodyCancelled = vi.fn();
    const response = cancellableResponse(503, "4", bodyCancelled);

    await expect(
      downloadModelFile({
        source: "https://example.test/model.gguf",
        targetPath,
        maxBytes: 8,
        signal: new AbortController().signal,
        onProgress: vi.fn(),
        fetcher: () => Promise.resolve(response),
      }),
    ).rejects.toThrow("model_download_http_failed");
    expect(bodyCancelled).toHaveBeenCalledOnce();
  });

  it.each([undefined, "", "NaN", "1.5", "-1", "0", "9", "9007199254740992"])(
    "rejects unsafe content-length %s and cancels the response body",
    async (contentLength) => {
      const targetPath = await createTargetPath();
      const bodyCancelled = vi.fn();
      const response = cancellableResponse(200, contentLength, bodyCancelled);

      await expect(
        downloadModelFile({
          source: "https://example.test/model.gguf",
          targetPath,
          maxBytes: 8,
          signal: new AbortController().signal,
          onProgress: vi.fn(),
          fetcher: () => Promise.resolve(response),
        }),
      ).rejects.toThrow("model_download_length_invalid");
      expect(bodyCancelled).toHaveBeenCalledOnce();
    },
  );

  it("rejects a body shorter than content-length", async () => {
    const targetPath = await createTargetPath();

    await expect(
      downloadModelFile({
        source: "https://example.test/model.gguf",
        targetPath,
        maxBytes: 8,
        signal: new AbortController().signal,
        onProgress: vi.fn(),
        fetcher: () => Promise.resolve(responseFromChunks([Uint8Array.from([1, 2])], "3")),
      }),
    ).rejects.toThrow("model_download_length_mismatch");
  });

  it("rejects a body that exceeds content-length before reporting excess progress", async () => {
    const targetPath = await createTargetPath();
    const progress = vi.fn();

    await expect(
      downloadModelFile({
        source: "https://example.test/model.gguf",
        targetPath,
        maxBytes: 8,
        signal: new AbortController().signal,
        onProgress: progress,
        fetcher: () => Promise.resolve(responseFromChunks([Uint8Array.from([1, 2, 3, 4])], "3")),
      }),
    ).rejects.toThrow("model_download_length_mismatch");
    expect(progress).not.toHaveBeenCalled();
  });

  it("cancels a request while fetch is waiting for response headers", async () => {
    const targetPath = await createTargetPath();
    const controller = new AbortController();
    let requestAborted = false;
    const fetcher: typeof globalThis.fetch = (_input, init) =>
      new Promise((_resolve, reject) => {
        init?.signal?.addEventListener(
          "abort",
          () => {
            requestAborted = true;
            reject(new Error("fetch aborted", { cause: init.signal?.reason }));
          },
          { once: true },
        );
      });

    const download = downloadModelFile({
      source: "https://example.test/model.gguf",
      targetPath,
      maxBytes: 8,
      signal: controller.signal,
      onProgress: vi.fn(),
      fetcher,
    });
    controller.abort();

    await expect(download).rejects.toThrow("model_download_cancelled");
    expect(requestAborted).toBe(true);
  });

  it("retries transient request failures before opening the staging file", async () => {
    const targetPath = await createTargetPath();
    const content = Uint8Array.from([1, 2, 3]);
    const fetcher = vi
      .fn<typeof globalThis.fetch>()
      .mockRejectedValueOnce(new TypeError("fetch failed"))
      .mockRejectedValueOnce(new TypeError("fetch failed"))
      .mockResolvedValueOnce(responseFromChunks([content], "3"));

    await downloadModelFile({
      source: "https://example.test/model.gguf",
      targetPath,
      maxBytes: 8,
      signal: new AbortController().signal,
      onProgress: vi.fn(),
      fetcher,
    });

    expect(fetcher).toHaveBeenCalledTimes(3);
    await expect(readFile(targetPath)).resolves.toEqual(Buffer.from(content));
  });

  it("does not retry a transient request failure after cancellation", async () => {
    const targetPath = await createTargetPath();
    const controller = new AbortController();
    const fetcher = vi.fn<typeof globalThis.fetch>(() => {
      controller.abort();
      return Promise.reject(new TypeError("fetch failed"));
    });

    await expect(
      downloadModelFile({
        source: "https://example.test/model.gguf",
        targetPath,
        maxBytes: 8,
        signal: controller.signal,
        onProgress: vi.fn(),
        fetcher,
      }),
    ).rejects.toThrow("model_download_cancelled");
    expect(fetcher).toHaveBeenCalledOnce();
  });

  it("cancels the response body and file pipeline during streaming", async () => {
    const targetPath = await createTargetPath();
    const controller = new AbortController();
    const bodyCancelled = vi.fn();
    const body = new ReadableStream<Uint8Array>({
      start(stream) {
        stream.enqueue(Uint8Array.from([1, 2, 3]));
      },
      cancel: bodyCancelled,
    });

    const download = downloadModelFile({
      source: "https://example.test/model.gguf",
      targetPath,
      maxBytes: 8,
      signal: controller.signal,
      onProgress: () => {
        controller.abort();
      },
      fetcher: () =>
        Promise.resolve(new Response(body, { status: 200, headers: { "content-length": "6" } })),
    });

    await expect(download).rejects.toThrow("model_download_cancelled");
    expect(bodyCancelled).toHaveBeenCalledOnce();
  });
});

const createTargetPath = async (): Promise<string> => {
  const directory = await mkdtemp(join(tmpdir(), "lyonbrown-model-download-"));
  temporaryDirectories.push(directory);
  return join(directory, "model.gguf");
};

const responseFromChunks = (chunks: Uint8Array[], contentLength: string): Response =>
  new Response(
    new ReadableStream<Uint8Array>({
      start(stream) {
        for (const chunk of chunks) stream.enqueue(chunk);
        stream.close();
      },
    }),
    { status: 200, headers: { "content-length": contentLength } },
  );

const cancellableResponse = (
  status: number,
  contentLength: string | undefined,
  cancelled: () => void,
): Response => {
  const init: ResponseInit =
    contentLength === undefined
      ? { status }
      : { status, headers: { "content-length": contentLength } };
  return new Response(new ReadableStream<Uint8Array>({ cancel: cancelled }), init);
};
