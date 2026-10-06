import { describe, expect, it, vi } from "vitest";

import type { LocalModelInferenceEvent, LocalModelInferenceRequest } from "../src/index.ts";
import { createNodeLlamaEngine } from "../src/internal/node-llama.ts";
import {
  createNodeLlamaHarness,
  type GenerationOptions,
  type GenerationResult,
} from "./support/node-llama.ts";

const collect = async (
  stream: AsyncIterable<LocalModelInferenceEvent>,
): Promise<LocalModelInferenceEvent[]> => {
  const events: LocalModelInferenceEvent[] = [];
  for await (const event of stream) events.push(event);
  return events;
};

const request = (
  signal = new AbortController().signal,
  overrides: Partial<LocalModelInferenceRequest> = {},
): LocalModelInferenceRequest => ({
  messages: [
    { role: "system", content: "Be concise." },
    { role: "user", content: "Hello" },
    { role: "assistant", content: "Hi" },
  ],
  maxOutputTokens: 64,
  maxOutputBytes: 1_024,
  signal,
  ...overrides,
});

describe("node-llama inference stream", () => {
  it("maps bounded messages and emits deltas, usage, and completion", async () => {
    const generateResponse = vi.fn(
      (_history: readonly unknown[], options: GenerationOptions): Promise<GenerationResult> => {
        options.onTextChunk("hel");
        options.onTextChunk("lo");
        return Promise.resolve({ metadata: { stopReason: "eogToken" } });
      },
    );
    const harness = createNodeLlamaHarness(generateResponse);
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );

    await expect(collect(lease.stream(request()))).resolves.toEqual([
      { type: "text_delta", text: "hel" },
      { type: "text_delta", text: "lo" },
      { type: "usage", inputTokens: 3, outputTokens: 2 },
      { type: "done", reason: "stop" },
    ]);
    expect(generateResponse).toHaveBeenCalledWith(
      [
        { type: "system", text: "Be concise." },
        { type: "user", text: "Hello" },
        { type: "model", response: ["Hi"] },
      ],
      expect.objectContaining({ maxTokens: 64, stopOnAbortSignal: true }),
    );
    const contextOptions = harness.createContext.mock.calls[0]?.[0];
    expect(contextOptions).toMatchObject({ contextSize: 4_096, sequences: 1 });
    expect(contextOptions?.createSignal).toBeInstanceOf(AbortSignal);
    expect(harness.disposeChat).toHaveBeenCalledOnce();
    expect(harness.disposeSequence).toHaveBeenCalledOnce();
    expect(harness.disposeContext).toHaveBeenCalledOnce();
    await lease.dispose();
  });

  it("does not allocate native resources for a pre-cancelled request", async () => {
    const harness = createNodeLlamaHarness(() =>
      Promise.resolve({ metadata: { stopReason: "eogToken" } }),
    );
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );
    const controller = new AbortController();
    controller.abort();

    await expect(collect(lease.stream(request(controller.signal)))).resolves.toEqual([
      { type: "done", reason: "cancelled" },
    ]);
    expect(harness.createContext).not.toHaveBeenCalled();
    await lease.dispose();
  });

  it("cancels generation in progress and releases stream resources", async () => {
    const harness = createNodeLlamaHarness(
      (_history, options) =>
        new Promise<GenerationResult>((resolve) => {
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
    const controller = new AbortController();
    const iterator = lease.stream(request(controller.signal))[Symbol.asyncIterator]();

    await expect(iterator.next()).resolves.toEqual({
      value: { type: "text_delta", text: "first" },
      done: false,
    });
    controller.abort();
    await expect(iterator.next()).resolves.toEqual({
      value: { type: "usage", inputTokens: 3, outputTokens: 2 },
      done: false,
    });
    await expect(iterator.next()).resolves.toEqual({
      value: { type: "done", reason: "cancelled" },
      done: false,
    });
    await expect(iterator.next()).resolves.toEqual({ value: undefined, done: true });
    expect(harness.disposeContext).toHaveBeenCalledOnce();
    await lease.dispose();
  });

  it("truncates at a UTF-8 byte boundary and actively stops native generation", async () => {
    let nativeSignal: AbortSignal | undefined;
    const harness = createNodeLlamaHarness((_history, options) => {
      nativeSignal = options.signal;
      options.onTextChunk("A你B");
      options.onTextChunk("ignored");
      return Promise.resolve({ metadata: { stopReason: "abort" } });
    });
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );

    await expect(collect(lease.stream(request(undefined, { maxOutputBytes: 4 })))).resolves.toEqual(
      [
        { type: "text_delta", text: "A你" },
        { type: "usage", inputTokens: 3, outputTokens: 2 },
        { type: "done", reason: "length" },
      ],
    );
    expect(nativeSignal?.aborted).toBe(true);
    await lease.dispose();
  });

  it("emits a sanitized terminal error and releases resources", async () => {
    const harness = createNodeLlamaHarness(() =>
      Promise.reject(new Error("C:\\Users\\alice\\secret.gguf failed")),
    );
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );

    const events = await collect(lease.stream(request()));

    expect(events).toEqual([
      {
        type: "error",
        code: "native_inference_failed",
        message: "Local model inference failed.",
      },
    ]);
    expect(JSON.stringify(events)).not.toContain("alice");
    expect(harness.disposeChat).toHaveBeenCalledOnce();
    expect(harness.disposeSequence).toHaveBeenCalledOnce();
    expect(harness.disposeContext).toHaveBeenCalledOnce();
    await lease.dispose();
  });

  it("rejects unsupported tool-shaped messages without touching native code", async () => {
    const harness = createNodeLlamaHarness(() =>
      Promise.resolve({ metadata: { stopReason: "eogToken" } }),
    );
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );
    const unsafeRequest = {
      ...request(),
      messages: [{ role: "tool", content: "do not execute me" }],
    } as unknown as LocalModelInferenceRequest;

    await expect(collect(lease.stream(unsafeRequest))).resolves.toEqual([
      {
        type: "error",
        code: "unsupported_tool_use",
        message: "Tool messages are not supported by the local model.",
      },
    ]);
    expect(harness.createContext).not.toHaveBeenCalled();
    await lease.dispose();
  });

  it("rejects a malformed cancellation signal before native allocation", async () => {
    const harness = createNodeLlamaHarness(() =>
      Promise.resolve({ metadata: { stopReason: "eogToken" } }),
    );
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );
    const unsafeRequest = {
      ...request(),
      signal: { aborted: false },
    } as unknown as LocalModelInferenceRequest;

    await expect(collect(lease.stream(unsafeRequest))).resolves.toEqual([
      {
        type: "error",
        code: "invalid_request",
        message: "Local model inference request is invalid.",
      },
    ]);
    expect(harness.createContext).not.toHaveBeenCalled();
    await lease.dispose();
  });

  it("sanitizes stream resource cleanup failures", async () => {
    const harness = createNodeLlamaHarness(() =>
      Promise.resolve({ metadata: { stopReason: "eogToken" } }),
    );
    harness.disposeContext.mockRejectedValueOnce(new Error("C:\\private\\context"));
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
    await lease.dispose();
  });

  it("rejects new inference after the lease is disposed", async () => {
    const harness = createNodeLlamaHarness(() =>
      Promise.resolve({ metadata: { stopReason: "eogToken" } }),
    );
    const lease = await createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", harness.api).loadDefault(
      new AbortController().signal,
    );
    await lease.dispose();

    await expect(collect(lease.stream(request()))).resolves.toEqual([
      { type: "error", code: "disposed", message: "Local model is disposed." },
    ]);
    expect(harness.createContext).not.toHaveBeenCalled();
  });
});
