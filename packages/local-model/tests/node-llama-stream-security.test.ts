import { describe, expect, it } from "vitest";

import type {
  LocalModelInferenceEvent,
  LocalModelInferenceRequest,
} from "@lyonbrown/local-model/internal/index";
import { createNodeLlamaEngine } from "@lyonbrown/local-model/internal/internal/node-llama";
import { createNodeLlamaHarness } from "@lyonbrown/local-model/test/support/node-llama";

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
  it.each([
    [[{ functionName: "unknown", params: {} }]],
    [[{ functionName: "read", params: new Date() }]],
    [[{ params: {} }]],
  ])("rejects untrusted native function call payload %#", async (functionCalls) => {
    const harness = createNodeLlamaHarness(() =>
      Promise.resolve({ metadata: { stopReason: "functionCalls" }, functionCalls }),
    );
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );

    await expect(
      collect(
        lease.stream({
          ...request(),
          tools: [{ name: "read", description: "Read", inputSchema: { type: "object" } }],
        }),
      ),
    ).resolves.toEqual([
      {
        type: "error",
        code: "native_inference_failed",
        message: "Local model inference failed.",
      },
    ]);
    await lease.dispose();
  });

  it("fails closed when native generation reports function calls without validated calls", async () => {
    const harness = createNodeLlamaHarness(() =>
      Promise.resolve({ metadata: { stopReason: "functionCalls" } }),
    );
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );

    await expect(collect(lease.stream(request()))).resolves.toEqual([
      {
        type: "error",
        code: "native_inference_failed",
        message: "Local model inference failed.",
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
