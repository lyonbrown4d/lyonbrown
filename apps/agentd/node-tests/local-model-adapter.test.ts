import type { LocalModelLease } from "@lyonbrown/local-model";
import type { KernelModelRequest } from "@lyonbrown/kernel-pi";
import { describe, expect, it, vi } from "vitest";

import { createLocalKernelModelAdapter } from "@lyonbrown/agentd/internal/local-model-adapter";

function request(overrides: Partial<KernelModelRequest> = {}): KernelModelRequest {
  return {
    sessionId: "session",
    model: { provider: "local", id: "model" },
    messages: [
      { role: "system", content: "be useful" },
      { role: "user", content: "hello" },
      { role: "assistant", content: "hi", toolCalls: [] },
    ],
    tools: [],
    signal: new AbortController().signal,
    maxOutputBytes: 4_096,
    ...overrides,
  };
}

async function collect(
  adapter: ReturnType<typeof createLocalKernelModelAdapter>,
  value = request(),
) {
  const events = [];
  for await (const event of adapter.stream(value)) events.push(event);
  return events;
}

describe("local model kernel adapter", () => {
  it("maps normalized messages and stream events", async () => {
    let seen: Parameters<LocalModelLease["stream"]>[0] | undefined;
    const lease: LocalModelLease = {
      async *stream(value) {
        seen = value;
        yield { type: "text_delta", text: "ok" };
        yield { type: "usage", inputTokens: 2, outputTokens: 1 };
        yield { type: "done", reason: "length" };
      },
      dispose: () => Promise.resolve(),
    };
    const adapter = createLocalKernelModelAdapter({ id: "model", lease });

    await expect(collect(adapter)).resolves.toEqual([
      { type: "text_delta", delta: "ok" },
      { type: "usage", usage: { inputTokens: 2, outputTokens: 1, totalTokens: 3 } },
      { type: "done", reason: "length" },
    ]);
    expect(seen).toMatchObject({ maxOutputTokens: 2_048, maxOutputBytes: 4_096 });
    expect(seen?.messages).toEqual([
      { role: "system", content: "be useful" },
      { role: "user", content: "hello" },
      { role: "assistant", content: "hi" },
    ]);
  });

  it("fails closed before touching the model for tool-shaped input", async () => {
    let called = false;
    const lease: LocalModelLease = {
      async *stream() {
        called = true;
        yield* [];
      },
      dispose: () => Promise.resolve(),
    };
    const adapter = createLocalKernelModelAdapter({ id: "model", lease });

    const events = await collect(
      adapter,
      request({
        tools: [{ name: "shell", description: "unsafe", inputSchema: {} }],
      }),
    );
    expect(events).toEqual([
      { type: "error", code: "model_error", message: "local model request rejected" },
    ]);
    expect(called).toBe(false);
  });

  it("maps cancellation and redacts every other model error", async () => {
    const lease: LocalModelLease = {
      async *stream() {
        yield { type: "error", code: "native_inference_failed", message: "C:\\secret.gguf" };
      },
      dispose: () => Promise.resolve(),
    };
    const adapter = createLocalKernelModelAdapter({ id: "model", lease });
    await expect(collect(adapter)).resolves.toEqual([
      { type: "error", code: "model_error", message: "local model inference failed" },
    ]);

    const cancelled: LocalModelLease = {
      async *stream() {
        yield { type: "done", reason: "cancelled" };
      },
      dispose: () => Promise.resolve(),
    };
    await expect(
      collect(createLocalKernelModelAdapter({ id: "model", lease: cancelled })),
    ).resolves.toEqual([
      { type: "error", code: "aborted", message: "local model inference cancelled" },
    ]);
  });

  it("stops consuming an untrusted lease after its first terminal event", async () => {
    const afterTerminal = vi.fn();
    const lease: LocalModelLease = {
      async *stream() {
        yield { type: "done", reason: "stop" };
        afterTerminal();
        yield { type: "text_delta", text: "must not escape" };
      },
      dispose: () => Promise.resolve(),
    };

    await expect(collect(createLocalKernelModelAdapter({ id: "model", lease }))).resolves.toEqual([
      { type: "done", reason: "stop" },
    ]);
    expect(afterTerminal).not.toHaveBeenCalled();
  });
});
