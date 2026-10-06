import { describe, expect, it, vi } from "vitest";

import type { LocalModelInferenceEvent } from "../src/index.ts";
import { createNodeLlamaEngine } from "../src/internal/node-llama.ts";
import { Deferred } from "./support/fakes.ts";

const collect = async (
  stream: AsyncIterable<LocalModelInferenceEvent>,
): Promise<LocalModelInferenceEvent[]> => {
  const events: LocalModelInferenceEvent[] = [];
  for await (const event of stream) events.push(event);
  return events;
};

describe("node-llama lease lifecycle", () => {
  it("aborts and waits for active streams before disposing native resources, idempotently", async () => {
    vi.useFakeTimers();
    const started = new Deferred<void>();
    const finishAfterAbort = new Deferred<void>();
    const disposeSequence = vi.fn(() => Promise.resolve());
    const disposeContext = vi.fn(() => Promise.resolve());
    const disposeModel = vi.fn(() => Promise.resolve());
    const disposeLlama = vi.fn(() => Promise.resolve());
    let generationSignal: AbortSignal | undefined;
    const engine = createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", {
      getLlama: () =>
        Promise.resolve({
          loadModel: () =>
            Promise.resolve({
              createContext: () =>
                Promise.resolve({
                  getSequence: () => ({
                    tokenMeter: {
                      getState: () => ({ usedInputTokens: 0, usedOutputTokens: 0 }),
                      diff: () => ({ usedInputTokens: 1, usedOutputTokens: 0 }),
                    },
                    dispose: disposeSequence,
                  }),
                  dispose: disposeContext,
                }),
              dispose: disposeModel,
            }),
          dispose: disposeLlama,
        }),
      createChat: () => ({
        generateResponse: async (
          _history: readonly unknown[],
          options: {
            readonly signal: AbortSignal;
            readonly onTextChunk: (text: string) => void;
            readonly maxTokens: number;
            readonly stopOnAbortSignal: true;
          },
        ) => {
          generationSignal = options.signal;
          started.resolve();
          await finishAfterAbort.promise;
          return { metadata: { stopReason: "abort" as const } };
        },
        dispose: vi.fn(),
      }),
    });
    const lease = await engine.loadDefault(new AbortController().signal);
    const stream = collect(
      lease.stream({
        messages: [{ role: "user", content: "hello" }],
        signal: new AbortController().signal,
        maxOutputTokens: 16,
        maxOutputBytes: 256,
      }),
    );
    await started.promise;

    try {
      const firstDispose = lease.dispose();
      const secondDispose = lease.dispose();
      expect(generationSignal?.aborted).toBe(true);
      const disposeResults = Promise.allSettled([firstDispose, secondDispose]);
      await vi.advanceTimersByTimeAsync(10_000);
      const settled = await disposeResults;
      const rejectionMessages = settled.map((result) => {
        if (result.status !== "rejected") throw new Error("dispose unexpectedly fulfilled");
        const reason: unknown = result.reason;
        return reason instanceof Error ? reason.message : "non_error_rejection";
      });
      expect(rejectionMessages).toEqual([
        "native_runtime_shutdown_timeout",
        "native_runtime_shutdown_timeout",
      ]);
      expect(disposeModel).not.toHaveBeenCalled();
      expect(disposeLlama).not.toHaveBeenCalled();

      finishAfterAbort.resolve();
      await expect(stream).resolves.toEqual([
        { type: "usage", inputTokens: 1, outputTokens: 0 },
        { type: "done", reason: "cancelled" },
      ]);
      await vi.advanceTimersByTimeAsync(0);
      expect(disposeSequence).toHaveBeenCalledOnce();
      expect(disposeContext).toHaveBeenCalledOnce();
      expect(disposeModel).toHaveBeenCalledOnce();
      expect(disposeLlama).toHaveBeenCalledOnce();
      await expect(lease.dispose()).rejects.toThrow("native_runtime_shutdown_timeout");
      expect(disposeModel).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });
});
