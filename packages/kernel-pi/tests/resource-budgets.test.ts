import { describe, expect, test, vi } from "vitest";

import {
  createPiKernelSession,
  type KernelEvent,
  type KernelModelAdapter,
  type KernelModelStreamEvent,
  type KernelToolDefinition,
  type PiKernelSessionOptions,
} from "../src/index.ts";
import { KernelRuntimeError } from "../src/errors.ts";

const baseBudget = {
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

class OneShotModel implements KernelModelAdapter {
  readonly model = { provider: "offline", id: "resource-budget" };
  #calls = 0;

  constructor(private readonly response: readonly KernelModelStreamEvent[]) {}

  async *stream(): AsyncIterable<KernelModelStreamEvent> {
    await Promise.resolve();
    this.#calls += 1;
    yield* this.#calls === 1 ? this.response : [{ type: "done" as const, reason: "stop" as const }];
  }
}

function toolCallingModel(arguments_: Record<string, unknown>): KernelModelAdapter {
  return new OneShotModel([
    { type: "tool_call", callId: "call-1", name: "danger", arguments: arguments_ },
    { type: "done", reason: "tool_use" },
  ]);
}

const baseTool: KernelToolDefinition = {
  name: "danger",
  label: "Danger",
  description: "A bounded action",
  inputSchema: {
    type: "object",
    properties: { token: {} },
    required: ["token"],
    additionalProperties: false,
  },
  permissions: ["granted"],
  risk: "low",
  execute: () => Promise.resolve({ content: "ok" }),
};

function createOptions(
  model: KernelModelAdapter,
  tool: KernelToolDefinition | undefined,
  events: KernelEvent[],
): PiKernelSessionOptions {
  return {
    sessionId: "resource-budget",
    systemPrompt: "Use bounded resources.",
    model,
    toolRegistry: {
      list: () => (tool ? [tool] : []),
      get: (name) => (tool?.name === name ? tool : undefined),
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
    budget: baseBudget,
  };
}

describe("resource budgets", () => {
  test("rejects oversized input with a structured error", async () => {
    const events: KernelEvent[] = [];
    const session = createPiKernelSession(
      createOptions(new OneShotModel([{ type: "done", reason: "stop" }]), undefined, events),
    );
    await expect(session.startTurn("x".repeat(2_000))).rejects.toMatchObject({
      code: "INPUT_BUDGET_EXCEEDED",
    });
  });

  test("aborts output and aggregate tool results beyond their byte budgets", async () => {
    const outputEvents: KernelEvent[] = [];
    const outputOptions = createOptions(
      new OneShotModel([
        { type: "text_delta", delta: "y".repeat(2_000) },
        { type: "done", reason: "stop" },
      ]),
      undefined,
      outputEvents,
    );
    await expect(createPiKernelSession(outputOptions).startTurn("go")).rejects.toMatchObject({
      code: "OUTPUT_BUDGET_EXCEEDED",
    });
    expect(outputEvents.some((event) => event.type === "run_completed")).toBe(false);

    for (const execute of [
      () => Promise.resolve({ content: "z".repeat(500) }),
      () =>
        Promise.resolve({
          content: "ok",
          details: Object.fromEntries(
            Array.from({ length: 10 }, (_, index) => [`key_${String(index)}`, "v".repeat(30)]),
          ),
        }),
    ]) {
      const events: KernelEvent[] = [];
      const tool = { ...baseTool, execute };
      await expect(
        createPiKernelSession(
          createOptions(toolCallingModel({ token: "x" }), tool, events),
        ).startTurn("run"),
      ).rejects.toMatchObject({ code: "TOOL_OUTCOME_UNKNOWN" });
      expect(events).toContainEqual(
        expect.objectContaining({
          type: "tool_outcome_unknown",
          code: "TOOL_RESULT_BUDGET_EXCEEDED",
        }),
      );
      expect(events.some((event) => event.type === "tool_succeeded")).toBe(false);
    }
  });

  test("rejects aggregate model argument bytes before durable intent", async () => {
    const events: KernelEvent[] = [];
    const execute = vi.fn(() => Promise.resolve({ content: "must not run" }));
    const arguments_ = Object.fromEntries(
      Array.from({ length: 16 }, (_, index) => [`field_${String(index)}`, "x".repeat(64)]),
    );
    await expect(
      createPiKernelSession(
        createOptions(toolCallingModel(arguments_), { ...baseTool, execute }, events),
      ).startTurn("run"),
    ).rejects.toMatchObject({ code: "INPUT_BUDGET_EXCEEDED" });
    expect(execute).not.toHaveBeenCalled();
    expect(events.some((event) => event.type === "tool_started")).toBe(false);
  });

  test("rejects a secret that normalizes to empty before execution", async () => {
    const events: KernelEvent[] = [];
    const execute = vi.fn(() => Promise.resolve({ content: "must not run" }));
    const options = createOptions(
      toolCallingModel({ token: { kind: "secretRef", reference: "empty-after-sanitize" } }),
      { ...baseTool, execute },
      events,
    );
    options.secretResolver.resolve = () => Promise.resolve("\u001b[31m\u0001\u202e");

    await createPiKernelSession(options).startTurn("run");

    expect(execute).not.toHaveBeenCalled();
    expect(events.some((event) => event.type === "tool_started")).toBe(true);
    expect(events).toContainEqual(
      expect.objectContaining({ type: "tool_failed", code: "UNTRUSTED_VALUE_INVALID" }),
    );
    expect(JSON.stringify(events)).not.toContain("empty-after-sanitize");
  });

  test("counts resolved secrets against the aggregate input budget before execution", async () => {
    const events: KernelEvent[] = [];
    const execute = vi.fn(() => Promise.resolve({ content: "must not run" }));
    const references = Object.fromEntries(
      Array.from({ length: 8 }, (_, index) => [
        `secret_${String(index)}`,
        { kind: "secretRef", reference: `ref_${String(index)}` },
      ]),
    );
    const tool: KernelToolDefinition = {
      ...baseTool,
      inputSchema: { type: "object", additionalProperties: { type: "string" } },
      execute,
    };
    const options = createOptions(toolCallingModel(references), tool, events);
    options.secretResolver.resolve = (reference) =>
      Promise.resolve(`${reference}-`.padEnd(200, "s"));

    await createPiKernelSession(options).startTurn("run");

    expect(execute).not.toHaveBeenCalled();
    expect(events).toContainEqual(
      expect.objectContaining({ type: "tool_failed", code: "INPUT_BUDGET_EXCEEDED" }),
    );
  });

  test("does not invoke hostile KernelRuntimeError accessors after durable intent", async () => {
    const messageGetter = vi.fn(() => "raw-secret");
    const codeGetter = vi.fn(() => "MODEL_ERROR");
    const hostile = new KernelRuntimeError("MODEL_ERROR", "replace-me");
    Object.defineProperties(hostile, {
      code: { configurable: true, get: codeGetter },
      message: { configurable: true, get: messageGetter },
    });
    const events: KernelEvent[] = [];
    const tool = { ...baseTool, execute: () => Promise.reject(hostile) };

    await createPiKernelSession(
      createOptions(toolCallingModel({ token: "x" }), tool, events),
    ).startTurn("run");

    expect(codeGetter).not.toHaveBeenCalled();
    expect(messageGetter).not.toHaveBeenCalled();
    expect(events).toContainEqual(
      expect.objectContaining({
        type: "tool_failed",
        code: "MODEL_ERROR",
        message: "Tool execution failed",
      }),
    );
  });

  test("rejects hostile result and model event accessors without invoking them", async () => {
    const resultGetter = vi.fn(() => "must not be read");
    const hostileResult = Object.defineProperty({}, "content", {
      enumerable: true,
      get: resultGetter,
    });
    const resultEvents: KernelEvent[] = [];
    await expect(
      createPiKernelSession(
        createOptions(
          toolCallingModel({ token: "x" }),
          { ...baseTool, execute: () => Promise.resolve(hostileResult as { content: string }) },
          resultEvents,
        ),
      ).startTurn("run"),
    ).rejects.toMatchObject({ code: "TOOL_OUTCOME_UNKNOWN" });
    expect(resultGetter).not.toHaveBeenCalled();

    const eventGetter = vi.fn(() => "done");
    const hostileEvent = Object.defineProperty({}, "type", {
      enumerable: true,
      get: eventGetter,
    });
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "hostile-event" },
      async *stream() {
        await Promise.resolve();
        yield hostileEvent as KernelModelStreamEvent;
      },
    };
    await expect(
      createPiKernelSession(createOptions(model, undefined, [])).startTurn("go"),
    ).rejects.toMatchObject({ code: "MODEL_ERROR" });
    expect(eventGetter).not.toHaveBeenCalled();
  });

  test("rejects malformed usage from an untrusted model adapter", async () => {
    const events: KernelEvent[] = [];
    const model = new OneShotModel([
      {
        type: "usage",
        usage: { inputTokens: -1, outputTokens: Number.NaN, totalTokens: 0 },
      },
      { type: "done", reason: "stop" },
    ]);
    await expect(
      createPiKernelSession(createOptions(model, undefined, events)).startTurn("go"),
    ).rejects.toMatchObject({ code: "MODEL_ERROR" });
    expect(events.some((event) => event.type === "run_failed")).toBe(true);
  });
});
