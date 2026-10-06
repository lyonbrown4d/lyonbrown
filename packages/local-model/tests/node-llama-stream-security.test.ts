import { describe, expect, it } from "vitest";

import type { LocalModelInferenceEvent, LocalModelInferenceRequest } from "../src/index.ts";
import { createNodeLlamaEngine } from "../src/internal/node-llama.ts";
import { createNodeLlamaHarness } from "./support/node-llama.ts";

const request = (): LocalModelInferenceRequest => ({
  messages: [{ role: "user", content: "hello" }],
  signal: new AbortController().signal,
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

describe("node-llama stream authority boundaries", () => {
  it("rejects a native function-call stop even when no calls are returned", async () => {
    const harness = createNodeLlamaHarness(() =>
      Promise.resolve({ metadata: { stopReason: "functionCalls" } }),
    );
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );

    await expect(collect(lease.stream(request()))).resolves.toEqual([
      {
        type: "error",
        code: "unsupported_tool_use",
        message: "Tool calls are not supported by the local model.",
      },
    ]);
    expect(harness.disposeContext).toHaveBeenCalledOnce();
    await lease.dispose();
  });

  it("aborts generation and releases stream resources when the consumer returns early", async () => {
    let generationSignal: AbortSignal | undefined;
    const harness = createNodeLlamaHarness(
      (_history, options) =>
        new Promise((resolve) => {
          generationSignal = options.signal;
          options.onTextChunk("first");
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
    const iterator = lease.stream(request())[Symbol.asyncIterator]();
    await expect(iterator.next()).resolves.toEqual({
      value: { type: "text_delta", text: "first" },
      done: false,
    });

    await iterator.return?.();

    expect(generationSignal?.aborted).toBe(true);
    expect(harness.disposeChat).toHaveBeenCalledOnce();
    expect(harness.disposeSequence).toHaveBeenCalledOnce();
    expect(harness.disposeContext).toHaveBeenCalledOnce();
    await lease.dispose();
  });
});
