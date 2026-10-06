import { describe, expect, test, vi } from "vitest";

import {
  createPiKernelSession,
  type KernelEvent,
  type KernelModelAdapter,
  type KernelModelStreamEvent,
  type PiKernelSessionOptions,
} from "@lyonbrown/kernel-pi/internal/index";
import {
  inspectOwnDataProperties,
  type InspectionFailure,
} from "@lyonbrown/kernel-pi/internal/inspection";
import { sanitizeJson } from "@lyonbrown/kernel-pi/internal/sanitization";
import { normalizeTools } from "@lyonbrown/kernel-pi/internal/tool-normalization";

const limits = {
  maxDepth: 4,
  maxStringBytes: 128,
  maxCollectionEntries: 8,
  maxNodes: 64,
  maxTotalBytes: 1_024,
  totalBytesErrorCode: "INPUT_BUDGET_EXCEEDED" as const,
};

const budget = {
  maxInputBytes: 1_024,
  maxTurns: 2,
  maxSteps: 2,
  maxOutputBytes: 1_024,
  maxToolResultBytes: 256,
  maxValueDepth: 5,
  maxValueStringBytes: 256,
  maxCollectionEntries: 16,
  maxValueNodes: 128,
  toolTimeoutMs: 100,
};

function options(model: KernelModelAdapter, events: KernelEvent[]): PiKernelSessionOptions {
  return {
    sessionId: "bounded-inspection",
    systemPrompt: "safe",
    model,
    toolRegistry: { list: () => [], get: () => undefined, hasPermission: () => false },
    approvalGate: { request: () => Promise.resolve({ decision: "deny-once" }) },
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

describe("bounded object inspection", () => {
  test("fails closed for every bounded property inspection violation", () => {
    const failure = (reason: InspectionFailure): Error => new Error(reason);
    const proxy = new Proxy({}, {});
    const symbol = { [Symbol("unsafe")]: true };
    const accessor = Object.defineProperty({}, "unsafe", { get: () => true });

    expect(() => inspectOwnDataProperties(proxy, 1, failure)).toThrow("proxy");
    expect(() => inspectOwnDataProperties({ first: 1, second: 2 }, 1, failure)).toThrow(
      "too_many_keys",
    );
    expect(() => inspectOwnDataProperties(symbol, 1, failure)).toThrow("symbol_key");
    expect(() => inspectOwnDataProperties(accessor, 1, failure)).toThrow("accessor");
  });

  test("does not use bulk descriptor or entry maps during JSON traversal", () => {
    const descriptors = vi.spyOn(Object, "getOwnPropertyDescriptors").mockImplementation(() => {
      throw new Error("bulk descriptors forbidden");
    });
    const entries = vi.spyOn(Object, "entries").mockImplementation(() => {
      throw new Error("bulk entries forbidden");
    });
    let result: unknown;
    try {
      result = sanitizeJson({ safe: [1, "two"] }, limits);
    } finally {
      descriptors.mockRestore();
      entries.mockRestore();
    }
    expect(result).toEqual({ safe: [1, "two"] });
  });

  test("checks an over-wide object before inspecting any getter descriptors", () => {
    const getter = vi.fn(() => "must not run");
    const wide = Object.create(null) as Record<string, unknown>;
    Object.defineProperty(wide, "first", { enumerable: true, get: getter });
    for (let index = 0; index < 8; index += 1) wide[`key_${String(index)}`] = index;

    expect(() => sanitizeJson(wide, limits)).toThrow("collection width");
    expect(getter).not.toHaveBeenCalled();
  });

  test("checks over-wide model events and tool definitions before reading getters", async () => {
    const eventGetter = vi.fn(() => "text_delta");
    const wideEvent: Record<string, unknown> = { one: 1, two: 2, three: 3, four: 4 };
    Object.defineProperty(wideEvent, "type", { enumerable: true, get: eventGetter });
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "wide-event" },
      async *stream() {
        await Promise.resolve();
        yield wideEvent as unknown as KernelModelStreamEvent;
      },
    };
    await expect(createPiKernelSession(options(model, [])).startTurn("go")).rejects.toMatchObject({
      code: "MODEL_ERROR",
    });
    expect(eventGetter).not.toHaveBeenCalled();

    const toolGetter = vi.fn(() => "safe");
    const wideTool = {
      label: "Safe",
      description: "safe",
      inputSchema: { type: "object" },
      permissions: [],
      risk: "low",
      execute: () => Promise.resolve({ content: "ok" }),
      extra: true,
    };
    Object.defineProperty(wideTool, "name", { enumerable: true, get: toolGetter });
    const config = options(model, []);
    expect(() => normalizeTools([wideTool] as never, config)).toThrow("too many fields");
    expect(toolGetter).not.toHaveBeenCalled();
  });

  test("normalizes tools without bulk descriptor or entry maps", () => {
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "unused" },
      async *stream() {
        await Promise.resolve();
        yield { type: "done", reason: "stop" };
      },
    };
    const config = options(model, []);
    const tool = {
      name: "safe",
      label: "Safe",
      description: "safe",
      inputSchema: { type: "object", additionalProperties: false },
      permissions: [],
      risk: "low" as const,
      execute: () => Promise.resolve({ content: "ok" }),
    };
    const descriptors = vi.spyOn(Object, "getOwnPropertyDescriptors").mockImplementation(() => {
      throw new Error("bulk descriptors forbidden");
    });
    const entries = vi.spyOn(Object, "entries").mockImplementation(() => {
      throw new Error("bulk entries forbidden");
    });
    let normalized: ReturnType<typeof normalizeTools> = [];
    try {
      normalized = normalizeTools([tool], config);
    } finally {
      descriptors.mockRestore();
      entries.mockRestore();
    }
    expect(normalized[0]?.name).toBe("safe");
  });

  test("rejects extra model event keys", async () => {
    const event = { type: "text_delta", delta: "must not emit", extra: true };
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "extra-event-fields" },
      async *stream() {
        await Promise.resolve();
        yield event as unknown as KernelModelStreamEvent;
        yield { type: "done", reason: "stop" };
      },
    };
    const events: KernelEvent[] = [];

    await expect(
      createPiKernelSession(options(model, events)).startTurn("go"),
    ).rejects.toMatchObject({ code: "MODEL_ERROR" });

    expect(events.some((item) => item.type === "text_delta")).toBe(false);
  });

  test("rejects an unknown fixed model event type", async () => {
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "unknown-event" },
      async *stream() {
        await Promise.resolve();
        yield { type: "unknown" } as unknown as KernelModelStreamEvent;
      },
    };

    await expect(createPiKernelSession(options(model, [])).startTurn("go")).rejects.toMatchObject({
      code: "MODEL_ERROR",
    });
  });
});
