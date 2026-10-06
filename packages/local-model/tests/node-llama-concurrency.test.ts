import { describe, expect, it } from "vitest";

import type { LocalModelInferenceEvent, LocalModelInferenceRequest } from "../src/index.ts";
import { createNodeLlamaEngine } from "../src/internal/node-llama.ts";
import { Deferred } from "./support/fakes.ts";
import { createNodeLlamaHarness } from "./support/node-llama.ts";

const request = (signal = new AbortController().signal): LocalModelInferenceRequest => ({
  messages: [{ role: "user", content: "hello" }],
  signal,
  maxOutputTokens: 64,
  maxOutputBytes: 1_024,
});

const collect = async (
  stream: AsyncIterable<LocalModelInferenceEvent>,
): Promise<LocalModelInferenceEvent[]> => {
  const events: LocalModelInferenceEvent[] = [];
  for await (const event of stream) events.push(event);
  return events;
};

describe("node-llama inference concurrency", () => {
  it("rejects a second active stream without queuing and allows reuse after completion", async () => {
    const finishFirst = new Deferred<void>();
    let generationCount = 0;
    const harness = createNodeLlamaHarness(async (_history, options) => {
      generationCount += 1;
      options.onTextChunk(generationCount === 1 ? "first" : "next");
      if (generationCount === 1) await finishFirst.promise;
      return { metadata: { stopReason: "eogToken" } };
    });
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );
    const first = lease.stream(request())[Symbol.asyncIterator]();
    await expect(first.next()).resolves.toMatchObject({
      value: { type: "text_delta", text: "first" },
      done: false,
    });

    const cancelled = new AbortController();
    cancelled.abort();
    await expect(collect(lease.stream(request(cancelled.signal)))).resolves.toEqual([
      { type: "done", reason: "cancelled" },
    ]);
    await expect(collect(lease.stream(request()))).resolves.toEqual([
      {
        type: "error",
        code: "resource_busy",
        message: "Local model inference is already active.",
      },
    ]);
    expect(harness.createContext).toHaveBeenCalledOnce();

    finishFirst.resolve();
    await expect(first.next()).resolves.toMatchObject({ value: { type: "usage" }, done: false });
    await expect(first.next()).resolves.toEqual({
      value: { type: "done", reason: "stop" },
      done: false,
    });
    await expect(first.next()).resolves.toEqual({ value: undefined, done: true });

    await expect(collect(lease.stream(request()))).resolves.toEqual([
      { type: "text_delta", text: "next" },
      { type: "usage", inputTokens: 3, outputTokens: 2 },
      { type: "done", reason: "stop" },
    ]);
    expect(harness.createContext).toHaveBeenCalledTimes(2);
    await lease.dispose();
  });

  it("rejects a new stream as disposed while an active stream is shutting down", async () => {
    const harness = createNodeLlamaHarness(
      (_history, options) =>
        new Promise((resolve) => {
          options.onTextChunk("active");
          options.signal.addEventListener(
            "abort",
            () => {
              resolve({ metadata: { stopReason: "abort" } });
            },
            { once: true },
          );
        }),
    );
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );
    const first = lease.stream(request())[Symbol.asyncIterator]();
    await first.next();

    const disposal = lease.dispose();
    await expect(collect(lease.stream(request()))).resolves.toEqual([
      { type: "error", code: "disposed", message: "Local model is disposed." },
    ]);
    expect(harness.createContext).toHaveBeenCalledOnce();
    await expect(disposal).resolves.toBeUndefined();
  });
});
