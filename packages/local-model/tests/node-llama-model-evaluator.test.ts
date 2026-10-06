import { describe, expect, it, vi } from "vitest";

import { MODEL_CATALOG, modelDownloadUri } from "../src/internal/catalog.ts";
import { createNodeLlamaModelEvaluator } from "../src/internal/node-llama-model-evaluator.ts";

describe("node-llama hardware compatibility adapter", () => {
  it("scores only trusted catalog URIs against one bounded hardware snapshot", async () => {
    const dispose = vi.fn(() => Promise.resolve());
    const getVramState = vi.fn(() =>
      Promise.resolve({ total: 8_000, free: 6_000, unifiedSize: 0 }),
    );
    const getRamState = vi.fn(() => Promise.resolve({ total: 32_000, free: 24_000 }));
    const readGgufFileInfo = vi.fn((uri: string) => Promise.resolve({ uri }));
    const resolveAndScoreConfig = vi.fn(() =>
      Promise.resolve({ compatibilityScore: 0.8, bonusScore: 0.25 }),
    );
    const evaluator = createNodeLlamaModelEvaluator({
      getLlama: () => Promise.resolve({ getVramState, getRamState, dispose }),
      readGgufFileInfo,
      createInsights: () => Promise.resolve({ isSupportedByLlamaCpp: true, resolveAndScoreConfig }),
    });

    const result = await evaluator.evaluate(MODEL_CATALOG, new AbortController().signal);

    expect(result).toHaveLength(MODEL_CATALOG.length);
    expect(readGgufFileInfo.mock.calls.map(([uri]) => uri)).toEqual(
      MODEL_CATALOG.map(modelDownloadUri),
    );
    expect(getVramState).toHaveBeenCalledOnce();
    expect(getRamState).toHaveBeenCalledOnce();
    expect(resolveAndScoreConfig).toHaveBeenCalledTimes(MODEL_CATALOG.length);
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("omits an unreadable candidate and still returns other compatible models", async () => {
    const [first, second] = MODEL_CATALOG;
    const evaluator = createNodeLlamaModelEvaluator({
      getLlama: () =>
        Promise.resolve({
          getVramState: () => Promise.resolve({ total: 0, free: 0, unifiedSize: 0 }),
          getRamState: () => Promise.resolve({ total: 8_000, free: 6_000 }),
          dispose: () => Promise.resolve(),
        }),
      readGgufFileInfo: (uri) =>
        uri === modelDownloadUri(first)
          ? Promise.reject(new Error("network details"))
          : Promise.resolve({ uri }),
      createInsights: () =>
        Promise.resolve({
          isSupportedByLlamaCpp: true,
          resolveAndScoreConfig: () => Promise.resolve({ compatibilityScore: 0.7, bonusScore: 0 }),
        }),
    });

    const result = await evaluator.evaluate([first, second], new AbortController().signal);

    expect(result).toEqual([{ modelId: second.id, compatibilityScore: 0.7, bonusScore: 0 }]);
  });

  it("rejects catalogs above the bounded trusted-candidate limit before probing hardware", async () => {
    const getLlama = vi.fn(() => Promise.reject(new Error("must not run")));
    const evaluator = createNodeLlamaModelEvaluator({
      getLlama,
      readGgufFileInfo: () => Promise.reject(new Error("must not run")),
      createInsights: () => Promise.reject(new Error("must not run")),
    });

    await expect(
      evaluator.evaluate(
        Array.from({ length: 9 }, () => MODEL_CATALOG[0]),
        new AbortController().signal,
      ),
    ).rejects.toThrow("trusted_model_catalog_too_large");
    expect(getLlama).not.toHaveBeenCalled();
  });

  it("rejects an already cancelled recommendation before probing hardware", async () => {
    const controller = new AbortController();
    controller.abort();
    const getLlama = vi.fn(() => Promise.reject(new Error("must not run")));
    const evaluator = createNodeLlamaModelEvaluator({
      getLlama,
      readGgufFileInfo: () => Promise.reject(new Error("must not run")),
      createInsights: () => Promise.reject(new Error("must not run")),
    });

    await expect(evaluator.evaluate(MODEL_CATALOG, controller.signal)).rejects.toThrow(
      "model_selection_cancelled",
    );
    expect(getLlama).not.toHaveBeenCalled();
  });

  it("degrades to no scores when the native hardware probe is unavailable", async () => {
    const evaluator = createNodeLlamaModelEvaluator({
      getLlama: () => Promise.reject(new Error("native details")),
      readGgufFileInfo: () => Promise.reject(new Error("must not run")),
      createInsights: () => Promise.reject(new Error("must not run")),
    });

    await expect(evaluator.evaluate(MODEL_CATALOG, new AbortController().signal)).resolves.toEqual(
      [],
    );
  });

  it("omits unsupported GGUF models and contains native disposal failures", async () => {
    const dispose = vi.fn(() => Promise.reject(new Error("private native details")));
    const evaluator = createNodeLlamaModelEvaluator({
      getLlama: () =>
        Promise.resolve({
          getVramState: () => Promise.resolve({ total: 0, free: 0, unifiedSize: 0 }),
          getRamState: () => Promise.resolve({ total: 8_000, free: 6_000 }),
          dispose,
        }),
      readGgufFileInfo: () => Promise.resolve({}),
      createInsights: () =>
        Promise.resolve({
          isSupportedByLlamaCpp: false,
          resolveAndScoreConfig: () => Promise.reject(new Error("must not run")),
        }),
    });

    await expect(
      evaluator.evaluate([MODEL_CATALOG[0]], new AbortController().signal),
    ).resolves.toEqual([]);
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("fences a cancellation that arrives while metadata is being inspected", async () => {
    const controller = new AbortController();
    const evaluator = createNodeLlamaModelEvaluator({
      getLlama: () =>
        Promise.resolve({
          getVramState: () => Promise.resolve({ total: 0, free: 0, unifiedSize: 0 }),
          getRamState: () => Promise.resolve({ total: 8_000, free: 6_000 }),
        }),
      readGgufFileInfo: () => {
        controller.abort();
        return Promise.reject(new Error("cancelled"));
      },
      createInsights: () => Promise.reject(new Error("must not run")),
    });

    await expect(evaluator.evaluate([MODEL_CATALOG[0]], controller.signal)).resolves.toEqual([]);
  });
});
