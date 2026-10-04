import { describe, expect, test, vi } from "vitest";

import {
  createPiKernelSession,
  type KernelEvent,
  type KernelModelAdapter,
  type KernelModelRequest,
  type KernelModelStreamEvent,
  type KernelToolDefinition,
} from "../src/index.js";

const budget = {
  maxInputBytes: 16_384,
  maxTurns: 8,
  maxSteps: 8,
  maxOutputBytes: 16_384,
  maxToolResultBytes: 4_096,
  maxValueDepth: 8,
  maxValueStringBytes: 4_096,
  maxCollectionEntries: 128,
  maxValueNodes: 512,
  toolTimeoutMs: 1_000,
};

function textResponse(text: string): readonly KernelModelStreamEvent[] {
  return [
    { type: "text_delta", delta: text.slice(0, 2) },
    { type: "text_delta", delta: text.slice(2) },
    {
      type: "usage",
      usage: { inputTokens: 3, outputTokens: 2, totalTokens: 5 },
    },
    { type: "done", reason: "stop" },
  ];
}

class ScriptedModel implements KernelModelAdapter {
  readonly model = { provider: "offline", id: "deterministic" };
  readonly requests: KernelModelRequest[] = [];

  constructor(
    private readonly responses: readonly (
      | readonly KernelModelStreamEvent[]
      | ((request: KernelModelRequest) => AsyncIterable<KernelModelStreamEvent>)
    )[],
  ) {}

  async *stream(request: KernelModelRequest): AsyncIterable<KernelModelStreamEvent> {
    this.requests.push(request);
    const response = this.responses[this.requests.length - 1];
    if (!response) throw new Error("missing scripted response");
    if (typeof response === "function") {
      yield* response(request);
      return;
    }
    for (const event of response) {
      await Promise.resolve();
      yield event;
    }
  }
}

function createDependencies(
  model: KernelModelAdapter,
  tools: readonly KernelToolDefinition[] = [],
) {
  const events: KernelEvent[] = [];
  return {
    events,
    options: {
      sessionId: "session-1",
      systemPrompt: "You are a bounded test agent.",
      model,
      toolRegistry: {
        list: () => tools,
        get: (name: string) => tools.find((tool) => tool.name === name),
        hasPermission: () => true,
      },
      approvalGate: { request: () => Promise.resolve({ decision: "approve" as const }) },
      eventSink: {
        append: (event: KernelEvent) => {
          events.push(event);
          return Promise.resolve();
        },
      },
      secretResolver: { resolve: () => Promise.resolve("resolved-secret") },
      budget,
    },
  };
}

describe("Pi kernel integration", () => {
  test("streams normalized events through a real Pi Agent", async () => {
    const model = new ScriptedModel([textResponse("hello")]);
    const { events, options } = createDependencies(model);
    const session = createPiKernelSession(options);
    const subscribed: KernelEvent[] = [];
    session.subscribe((event) => void subscribed.push(event));

    await session.startTurn("hi");

    await vi.waitFor(() => {
      expect(subscribed).toHaveLength(events.length);
    });

    expect(events.filter((event) => event.type === "text_delta")).toEqual([
      { type: "text_delta", delta: "he" },
      { type: "text_delta", delta: "llo" },
    ]);
    expect(events.some((event) => event.type === "usage")).toBe(true);
    expect(subscribed).toEqual(events);
    expect(model.requests).toHaveLength(1);
  });

  test("executes a model tool batch sequentially and persists intent first", async () => {
    let active = 0;
    let maximumActive = 0;
    const executionOrder: string[] = [];
    const createTool = (name: string): KernelToolDefinition => ({
      name,
      label: name,
      description: `${name} description`,
      inputSchema: {
        type: "object",
        properties: { value: { type: "string" } },
        required: ["value"],
        additionalProperties: false,
      },
      permissions: ["test.execute"],
      risk: "low",
      execute: async ({ callId, arguments: arguments_ }) => {
        active += 1;
        maximumActive = Math.max(maximumActive, active);
        executionOrder.push(`execute:${callId}:${String(arguments_.value)}`);
        await Promise.resolve();
        active -= 1;
        return { content: `ok:${name}` };
      },
    });
    const model = new ScriptedModel([
      [
        { type: "tool_call", callId: "call-a", name: "alpha", arguments: { value: "a" } },
        { type: "tool_call", callId: "call-b", name: "beta", arguments: { value: "b" } },
        { type: "done", reason: "tool_use" },
      ],
      textResponse("done"),
    ]);
    const { events, options } = createDependencies(model, [
      createTool("alpha"),
      createTool("beta"),
    ]);
    options.eventSink = {
      append: (event) => {
        events.push(event);
        if (event.type === "tool_started") executionOrder.push(`persist:${event.callId}`);
        return Promise.resolve();
      },
    };

    await createPiKernelSession(options).startTurn("run both");

    expect(maximumActive).toBe(1);
    expect(executionOrder).toEqual([
      "persist:call-a",
      "execute:call-a:a",
      "persist:call-b",
      "execute:call-b:b",
    ]);
    expect(events.filter((event) => event.type === "tool_succeeded")).toHaveLength(2);
  });

  test("drains steering before follow-up and supports cancellation", async () => {
    let releaseFirst: (() => void) | undefined;
    const firstResponse = async function* (
      request: KernelModelRequest,
    ): AsyncIterable<KernelModelStreamEvent> {
      await new Promise<void>((resolve) => {
        releaseFirst = resolve;
        request.signal.addEventListener(
          "abort",
          () => {
            resolve();
          },
          { once: true },
        );
      });
      if (request.signal.aborted) {
        yield { type: "error", code: "aborted", message: "aborted" };
      } else {
        yield* textResponse("first");
      }
    };
    const model = new ScriptedModel([
      firstResponse,
      textResponse("steered"),
      textResponse("followed"),
    ]);
    const { options } = createDependencies(model);
    const session = createPiKernelSession(options);
    const run = session.startTurn("start");
    await vi.waitFor(() => {
      expect(releaseFirst).toBeTypeOf("function");
    });
    session.steer("change direction");
    session.followUp("then summarize");
    releaseFirst?.();
    await run;

    expect(model.requests).toHaveLength(3);
    expect(JSON.stringify(model.requests[1]?.messages)).toContain("change direction");
    expect(JSON.stringify(model.requests[2]?.messages)).toContain("then summarize");

    const cancelModel = new ScriptedModel([firstResponse]);
    const cancelSession = createPiKernelSession(createDependencies(cancelModel).options);
    const cancelled = cancelSession.startTurn("wait");
    await vi.waitFor(() => {
      expect(cancelModel.requests).toHaveLength(1);
    });
    cancelSession.cancel();
    await cancelled;
    expect(cancelModel.requests[0]?.signal.aborted).toBe(true);
  });

  test("fences tool continuation at maxTurns before invoking the adapter again", async () => {
    const model = new ScriptedModel([
      [
        { type: "tool_call", callId: "call-a", name: "alpha", arguments: { value: "a" } },
        { type: "done", reason: "tool_use" },
      ],
      textResponse("must not run"),
    ]);
    const tool: KernelToolDefinition = {
      name: "alpha",
      label: "Alpha",
      description: "One effect",
      inputSchema: {
        type: "object",
        properties: { value: { type: "string" } },
        required: ["value"],
        additionalProperties: false,
      },
      permissions: ["test.execute"],
      risk: "low",
      execute: () => Promise.resolve({ content: "ok" }),
    };
    const { events, options } = createDependencies(model, [tool]);
    options.budget = { ...budget, maxTurns: 1 };

    await expect(createPiKernelSession(options).startTurn("run")).rejects.toMatchObject({
      code: "TURN_BUDGET_EXCEEDED",
    });

    expect(model.requests).toHaveLength(1);
    expect(events.filter((event) => event.type === "turn_started")).toHaveLength(1);
    expect(events.filter((event) => event.type === "turn_completed")).toHaveLength(1);
    expect(events.filter((event) => event.type === "usage")).toHaveLength(1);
  });

  test.each(["steer", "followUp"] as const)(
    "fences queued %s input at maxTurns without another adapter call",
    async (mode) => {
      let release: (() => void) | undefined;
      const first = async function* (): AsyncIterable<KernelModelStreamEvent> {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
        yield* textResponse("first");
      };
      const model = new ScriptedModel([first, textResponse("must not run")]);
      const { events, options } = createDependencies(model);
      options.budget = { ...budget, maxTurns: 1 };
      const session = createPiKernelSession(options);
      const run = session.startTurn("start");
      await vi.waitFor(() => {
        expect(model.requests).toHaveLength(1);
      });
      session[mode]("queued");
      release?.();

      await expect(run).rejects.toMatchObject({ code: "TURN_BUDGET_EXCEEDED" });
      expect(model.requests).toHaveLength(1);
      expect(events.filter((event) => event.type === "turn_completed")).toHaveLength(1);
      expect(events.filter((event) => event.type === "usage")).toHaveLength(1);
    },
  );
});
