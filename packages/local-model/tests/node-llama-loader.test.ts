import { describe, expect, it, vi } from "vitest";

import { createNodeLlamaEngine } from "@lyonbrown/local-model/internal/internal/node-llama";

describe("node-llama-cpp production boundary", () => {
  it("loads only official prebuilds and forbids source downloads and compilation fallback", async () => {
    const dispose = vi.fn(() => Promise.resolve());
    const loadModel = vi.fn(() => Promise.resolve({ dispose }));
    const getLlama = vi.fn(() => Promise.resolve({ loadModel }));
    const engine = createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", { getLlama });

    const loadController = new AbortController();
    const lease = await engine.loadDefault(loadController.signal);

    expect(getLlama).toHaveBeenCalledWith({
      build: "never",
      skipDownload: true,
      usePrebuiltBinaries: true,
      progressLogs: false,
    });
    expect(getLlama.mock.results[0]?.value).toBeInstanceOf(Promise);
    expect(loadModel).toHaveBeenCalledWith(
      expect.objectContaining({ loadSignal: loadController.signal }),
    );
    await lease.dispose();
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("sanitizes binding acquisition failures", async () => {
    const engine = createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", {
      getLlama: () => Promise.reject(new Error("C:\\Users\\alice\\private\\binding")),
    });

    await expect(engine.loadDefault(new AbortController().signal)).rejects.toThrow(
      "native_runtime_load_failed",
    );
  });

  it("sanitizes model loading and native disposal failures", async () => {
    const disposeLlama = vi
      .fn(() => Promise.resolve())
      .mockRejectedValueOnce(new Error("C:\\private\\binding"));
    const engine = createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", {
      getLlama: () =>
        Promise.resolve({
          loadModel: () => Promise.reject(new Error("C:\\private\\model")),
          dispose: disposeLlama,
        }),
    });
    await expect(engine.loadDefault(new AbortController().signal)).rejects.toThrow(
      "native_runtime_load_failed",
    );
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
    }).loadDefault(new AbortController().signal);
    await expect(failingLease.dispose()).rejects.toThrow("native_runtime_dispose_failed");
  });
});
