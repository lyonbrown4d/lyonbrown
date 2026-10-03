import { describe, expect, test, vi } from "vitest";

import { KernelRuntimeError, kernelError, toPublicKernelError } from "../src/errors.js";
import {
  createPiKernelSession,
  type KernelEvent,
  type KernelModelAdapter,
  type KernelModelStreamEvent,
  type KernelToolDefinition,
  type PiKernelSessionOptions,
} from "../src/index.js";

const budget = {
  maxInputBytes: 1_024,
  maxTurns: 3,
  maxSteps: 3,
  maxOutputBytes: 1_024,
  maxToolResultBytes: 256,
  maxValueDepth: 5,
  maxValueStringBytes: 256,
  maxCollectionEntries: 32,
  maxValueNodes: 128,
  toolTimeoutMs: 100,
};

function options(
  model: KernelModelAdapter,
  events: KernelEvent[],
  tool?: KernelToolDefinition,
): PiKernelSessionOptions {
  return {
    sessionId: "external-errors",
    systemPrompt: "safe",
    model,
    toolRegistry: {
      list: () => (tool ? [tool] : []),
      get: (name) => (name === tool?.name ? tool : undefined),
      hasPermission: () => true,
    },
    approvalGate: { request: () => Promise.resolve({ decision: "approve" }) },
    eventSink: {
      append: (event) => {
        events.push(event);
        return Promise.resolve();
      },
    },
    secretResolver: { resolve: () => Promise.resolve("secret") },
    budget,
  };
}

function throwingModel(error: unknown): KernelModelAdapter {
  return {
    model: { provider: "offline", id: "throwing" },
    stream(): AsyncIterable<KernelModelStreamEvent> {
      return {
        [Symbol.asyncIterator]: () => ({
          next: async () => {
            await Promise.resolve();
            throw error;
          },
        }),
      };
    },
  };
}

describe("external error boundaries", () => {
  test("only preserves frozen errors created by the internal factory", () => {
    const internal = kernelError("STEP_BUDGET_EXCEEDED", "bounded");
    expect(Object.isFrozen(internal)).toBe(true);
    expect(toPublicKernelError(internal)).toMatchObject({
      code: "STEP_BUDGET_EXCEEDED",
      message: "bounded",
    });
    expect(
      toPublicKernelError(new KernelRuntimeError("TOOL_DENIED", "external-secret")),
    ).toMatchObject({ code: "INTERNAL", message: "Kernel runtime failed" });
    expect(kernelError("MODEL_ERROR", "x".repeat(5_000))).toMatchObject({
      code: "INTERNAL",
      message: "Kernel runtime failed",
    });
    const get = vi.fn((): never => {
      throw new Error("Proxy trap must not run");
    });
    const proxy = new Proxy(new KernelRuntimeError("MODEL_ERROR", "external-secret"), { get });
    expect(toPublicKernelError(proxy)).toMatchObject({
      code: "INTERNAL",
      message: "Kernel runtime failed",
    });
    expect(get).not.toHaveBeenCalled();
  });

  test("maps unbranded and subclassed adapter errors to one fixed public error", async () => {
    class HostileRuntimeError extends KernelRuntimeError {}
    const code = vi.fn(() => "TOOL_DENIED");
    const message = vi.fn(() => "raw-adapter-secret");
    const hostile = new HostileRuntimeError("MODEL_ERROR", "replace-me");
    Object.defineProperties(hostile, {
      code: { configurable: true, get: code },
      message: { configurable: true, get: message },
    });

    for (const error of [new KernelRuntimeError("TOOL_DENIED", "raw-adapter-secret"), hostile]) {
      const events: KernelEvent[] = [];
      await expect(
        createPiKernelSession(options(throwingModel(error), events)).startTurn("go"),
      ).rejects.toMatchObject({ code: "MODEL_ERROR", message: "Model adapter failed" });
      expect(JSON.stringify(events)).not.toContain("raw-adapter-secret");
    }
    expect(code).not.toHaveBeenCalled();
    expect(message).not.toHaveBeenCalled();
  });

  test("maps model descriptor access errors without inspecting the thrown value", () => {
    const code = vi.fn(() => "MODEL_ERROR");
    const message = vi.fn(() => "raw-descriptor-secret");
    const hostile = new KernelRuntimeError("MODEL_ERROR", "replace-me");
    Object.defineProperties(hostile, {
      code: { configurable: true, get: code },
      message: { configurable: true, get: message },
    });
    const model = throwingModel(hostile);
    Object.defineProperty(model, "model", {
      get: () => {
        throw hostile;
      },
    });

    expect(() => createPiKernelSession(options(model, []))).toThrow(
      expect.objectContaining({ code: "MODEL_ERROR", message: "Model adapter failed" }),
    );
    expect(code).not.toHaveBeenCalled();
    expect(message).not.toHaveBeenCalled();
  });

  test("snapshots the model descriptor before a turn can mutate it", async () => {
    const provider = vi.fn((): never => {
      throw new Error("late-provider-secret");
    });
    const id = vi.fn((): never => {
      throw new Error("late-id-secret");
    });
    const descriptor = { provider: "offline", id: "snapshot" };
    const seen: unknown[] = [];
    const model: KernelModelAdapter = {
      model: descriptor,
      async *stream(request) {
        seen.push(request.model);
        await Promise.resolve();
        yield { type: "done", reason: "stop" };
      },
    };
    const session = createPiKernelSession(options(model, []));
    Object.defineProperties(descriptor, {
      provider: { configurable: true, get: provider },
      id: { configurable: true, get: id },
    });

    await session.startTurn("go");

    expect(seen).toEqual([{ provider: "offline", id: "snapshot" }]);
    expect(provider).not.toHaveBeenCalled();
    expect(id).not.toHaveBeenCalled();
  });

  test("maps tool registry list failures without inspecting thrown errors", () => {
    const code = vi.fn(() => "TOOL_DENIED");
    const message = vi.fn(() => "raw-registry-secret");
    const hostile = new KernelRuntimeError("MODEL_ERROR", "replace-me");
    Object.defineProperties(hostile, {
      code: { configurable: true, get: code },
      message: { configurable: true, get: message },
    });
    const proxyGet = vi.fn((): never => {
      throw new Error("Proxy trap must not run");
    });
    const proxied = new Proxy(new Error("raw-proxy-registry-secret"), { get: proxyGet });

    for (const error of [hostile, proxied]) {
      const config = options(throwingModel(new Error("unused")), []);
      config.toolRegistry.list = () => {
        throw error;
      };
      expect(() => createPiKernelSession(config)).toThrow(
        expect.objectContaining({ code: "INTERNAL", message: "Kernel runtime failed" }),
      );
    }
    expect(code).not.toHaveBeenCalled();
    expect(message).not.toHaveBeenCalled();
    expect(proxyGet).not.toHaveBeenCalled();
  });

  test("rejects proxied adapter errors without invoking traps", async () => {
    const get = vi.fn((): never => {
      throw new Error("Proxy trap must not run");
    });
    const hostile = new Proxy(new KernelRuntimeError("MODEL_ERROR", "raw-proxy-secret"), { get });
    const events: KernelEvent[] = [];

    await expect(
      createPiKernelSession(options(throwingModel(hostile), events)).startTurn("go"),
    ).rejects.toMatchObject({ code: "MODEL_ERROR", message: "Model adapter failed" });

    expect(get).not.toHaveBeenCalled();
    expect(JSON.stringify(events)).not.toContain("raw-proxy-secret");
  });

  test("does not expose an ordinary tool exception after durable intent", async () => {
    let calls = 0;
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "tool-error" },
      async *stream() {
        await Promise.resolve();
        calls += 1;
        if (calls === 1) {
          yield { type: "tool_call", callId: "call-1", name: "effect", arguments: {} };
          yield { type: "done", reason: "tool_use" };
        } else {
          yield { type: "done", reason: "stop" };
        }
      },
    };
    const tool: KernelToolDefinition = {
      name: "effect",
      label: "Effect",
      description: "safe",
      inputSchema: { type: "object", additionalProperties: false },
      permissions: [],
      risk: "low",
      execute: () => Promise.reject(new Error("raw-tool-secret")),
    };
    const events: KernelEvent[] = [];

    await createPiKernelSession(options(model, events, tool)).startTurn("run");

    expect(events).toContainEqual(
      expect.objectContaining({
        type: "tool_failed",
        code: "MODEL_ERROR",
        message: "Tool execution failed",
      }),
    );
    expect(JSON.stringify(events)).not.toContain("raw-tool-secret");
  });
});
