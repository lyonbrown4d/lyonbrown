import { describe, expect, it, vi } from "vitest";

import type {
  LocalModelInferenceEvent,
  LocalModelInferenceRequest,
} from "@lyonbrown/local-model/internal/index";
import { createNodeLlamaEngine } from "@lyonbrown/local-model/internal/internal/node-llama";
import { Deferred } from "@lyonbrown/local-model/test/support/fakes";

interface ContextOptions {
  readonly contextSize: 4_096;
  readonly sequences: 1;
  readonly createSignal: AbortSignal;
}

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

const createBlockedContextLease = async () => {
  const contextStarted = new Deferred<void>();
  const disposeModel = vi.fn(() => Promise.resolve());
  const disposeLlama = vi.fn(() => Promise.resolve());
  const createChat = vi.fn();
  let createSignal: AbortSignal | undefined;
  const createContext = vi.fn(
    (options?: ContextOptions) =>
      new Promise<never>((_resolve, reject) => {
        contextStarted.resolve();
        if (options === undefined) return;
        createSignal = options.createSignal;
        if (options.createSignal.aborted) {
          reject(new Error("context creation cancelled"));
          return;
        }
        options.createSignal.addEventListener(
          "abort",
          () => {
            reject(new Error("context creation cancelled"));
          },
          { once: true },
        );
      }),
  );
  const engine = createNodeLlamaEngine("C:\\trusted\\Qwen.gguf", {
    getLlama: () =>
      Promise.resolve({
        loadModel: () => Promise.resolve({ createContext, dispose: disposeModel }),
        dispose: disposeLlama,
      }),
    createChat,
  });
  const lease = await engine.loadDefault(new AbortController().signal);
  return {
    contextStarted,
    createChat,
    createContext,
    createSignal: () => createSignal,
    disposeLlama,
    disposeModel,
    lease,
  };
};

describe("node-llama context creation lifecycle", () => {
  it("aborts context creation when the request is cancelled", async () => {
    const harness = await createBlockedContextLease();
    const controller = new AbortController();
    const events = collect(harness.lease.stream(request(controller.signal)));
    await harness.contextStarted.promise;

    controller.abort();

    expect(harness.createSignal()?.aborted).toBe(true);
    await expect(events).resolves.toEqual([{ type: "done", reason: "cancelled" }]);
    expect(harness.createChat).not.toHaveBeenCalled();
    await harness.lease.dispose();
  });

  it("aborts context creation when the consumer returns with next pending", async () => {
    const harness = await createBlockedContextLease();
    const iterator = harness.lease.stream(request())[Symbol.asyncIterator]();
    const pendingNext = iterator.next();
    await harness.contextStarted.promise;
    if (iterator.return === undefined) throw new Error("iterator return unavailable");

    const returning = iterator.return();

    await Promise.resolve();
    expect(harness.createSignal()?.aborted).toBe(true);
    await expect(returning).resolves.toEqual({ value: undefined, done: true });
    await expect(pendingNext).resolves.toEqual({ value: undefined, done: true });
    expect(harness.createChat).not.toHaveBeenCalled();
    await harness.lease.dispose();
  });

  it("aborts context creation before lease disposal releases native resources", async () => {
    const harness = await createBlockedContextLease();
    const events = collect(harness.lease.stream(request()));
    await harness.contextStarted.promise;

    const disposal = harness.lease.dispose();

    expect(harness.createSignal()?.aborted).toBe(true);
    expect(harness.disposeModel).not.toHaveBeenCalled();
    expect(harness.disposeLlama).not.toHaveBeenCalled();
    await expect(events).resolves.toEqual([{ type: "done", reason: "cancelled" }]);
    await expect(disposal).resolves.toBeUndefined();
    expect(harness.disposeModel).toHaveBeenCalledOnce();
    expect(harness.disposeLlama).toHaveBeenCalledOnce();
  });
});
