import { describe, expect, test, vi } from "vitest";

import {
  createPiKernelSession,
  type KernelEvent,
  type KernelModelAdapter,
  type KernelModelRequest,
  type KernelModelStreamEvent,
  type KernelResourceBudget,
  type KernelToolDefinition,
  type PiKernelSessionOptions,
} from "@lyonbrown/kernel-pi/internal/index";

const budget: KernelResourceBudget = {
  maxInputBytes: 4_096,
  maxTurns: 4,
  maxSteps: 4,
  maxOutputBytes: 4_096,
  maxToolResultBytes: 1_024,
  maxValueDepth: 6,
  maxValueStringBytes: 1_024,
  maxCollectionEntries: 64,
  maxValueNodes: 256,
  toolTimeoutMs: 50,
};

class ToolThenStopModel implements KernelModelAdapter {
  readonly model = { provider: "offline", id: "durability" };
  calls = 0;

  async *stream(request: KernelModelRequest): AsyncIterable<KernelModelStreamEvent> {
    this.calls += 1;
    await Promise.resolve();
    if (request.signal.aborted) {
      yield { type: "error", code: "aborted", message: "raw abort" };
    } else if (this.calls === 1) {
      yield {
        type: "tool_call",
        callId: "call-1",
        name: "effect",
        arguments: { value: "safe" },
      };
      yield { type: "done", reason: "tool_use" };
    } else {
      yield { type: "done", reason: "stop" };
    }
  }
}

function createTool(execute: KernelToolDefinition["execute"]): KernelToolDefinition {
  return {
    name: "effect",
    label: "Effect",
    description: "Perform one bounded effect",
    inputSchema: {
      type: "object",
      properties: { value: { type: "string" } },
      required: ["value"],
      additionalProperties: false,
    },
    permissions: ["effect.run"],
    risk: "low",
    execute,
  };
}

function createOptions(
  model: KernelModelAdapter,
  tool: KernelToolDefinition,
  append: (event: KernelEvent) => Promise<void>,
  overrides: Partial<PiKernelSessionOptions> = {},
): PiKernelSessionOptions {
  return {
    sessionId: "durability-session",
    systemPrompt: "Use only registered tools.",
    model,
    toolRegistry: {
      list: () => [tool],
      get: (name) => (name === tool.name ? tool : undefined),
      hasPermission: () => true,
    },
    approvalGate: { request: () => Promise.resolve({ decision: "approve" }) },
    eventSink: { append },
    secretResolver: { resolve: () => Promise.resolve("resolved") },
    budget,
    ...overrides,
  };
}

describe("durable tool outcomes", () => {
  test("treats ToolStarted sink failure as fatal without executing or appending a fake failure", async () => {
    const attempted: KernelEvent["type"][] = [];
    const execute = vi.fn(() => Promise.resolve({ content: "ok" }));
    const model = new ToolThenStopModel();
    const append = (event: KernelEvent): Promise<void> => {
      attempted.push(event.type);
      return event.type === "tool_started"
        ? Promise.reject(new Error("database password=raw-secret"))
        : Promise.resolve();
    };

    const session = createPiKernelSession(createOptions(model, createTool(execute), append));
    await expect(session.startTurn("run")).rejects.toMatchObject({
      code: "EVENT_SINK_UNAVAILABLE",
    });
    await expect(session.startTurn("must reject")).rejects.toMatchObject({
      code: "EVENT_SINK_UNAVAILABLE",
    });
    expect(() => {
      session.followUp("must reject");
    }).toThrow(expect.objectContaining({ code: "EVENT_SINK_UNAVAILABLE" }));

    expect(execute).not.toHaveBeenCalled();
    expect(attempted).not.toContain("tool_failed");
    expect(attempted).not.toContain("run_completed");
    expect(model.calls).toBe(1);
  });

  test("returns typed outcome-unknown fatal when success cannot be persisted", async () => {
    const attempted: KernelEvent["type"][] = [];
    const execute = vi.fn(() => Promise.resolve({ content: "effect complete" }));
    const append = (event: KernelEvent): Promise<void> => {
      attempted.push(event.type);
      return event.type === "tool_succeeded"
        ? Promise.reject(new Error("sink exploded with raw-secret"))
        : Promise.resolve();
    };

    const run = createPiKernelSession(
      createOptions(new ToolThenStopModel(), createTool(execute), append),
    ).startTurn("run");
    await expect(run).rejects.toMatchObject({ code: "TOOL_OUTCOME_UNKNOWN" });
    await expect(run).rejects.not.toHaveProperty("cause");
    expect(execute).toHaveBeenCalledOnce();
    expect(attempted).not.toContain("tool_failed");
    expect(attempted).not.toContain("run_completed");
  });

  test("keeps an uncertain outcome terminal for the entire session", async () => {
    const model = new ToolThenStopModel();
    const execute = vi.fn(() => Promise.resolve({ content: "effect complete" }));
    const session = createPiKernelSession(
      createOptions(model, createTool(execute), (event) =>
        event.type === "tool_succeeded"
          ? Promise.reject(new Error("terminal durability failure"))
          : Promise.resolve(),
      ),
    );

    await expect(session.startTurn("run")).rejects.toMatchObject({
      code: "TOOL_OUTCOME_UNKNOWN",
    });
    const modelCalls = model.calls;
    const toolCalls = execute.mock.calls.length;

    await expect(session.startTurn("must reject")).rejects.toMatchObject({
      code: "TOOL_OUTCOME_UNKNOWN",
    });
    expect(() => {
      session.steer("must reject");
    }).toThrow(expect.objectContaining({ code: "TOOL_OUTCOME_UNKNOWN" }));
    expect(() => {
      session.followUp("must reject");
    }).toThrow(expect.objectContaining({ code: "TOOL_OUTCOME_UNKNOWN" }));
    expect(model.calls).toBe(modelCalls);
    expect(execute).toHaveBeenCalledTimes(toolCalls);
  });

  test("clears ordinary transient run failures before the next run", async () => {
    let calls = 0;
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "transient" },
      async *stream() {
        await Promise.resolve();
        calls += 1;
        if (calls === 1) throw new Error("temporary provider failure");
        yield { type: "done", reason: "stop" };
      },
    };
    const execute = vi.fn(() => Promise.resolve({ content: "unused" }));
    const session = createPiKernelSession(
      createOptions(model, createTool(execute), () => Promise.resolve()),
    );

    await expect(session.startTurn("first")).rejects.toMatchObject({ code: "MODEL_ERROR" });
    await expect(session.startTurn("second")).resolves.toBeUndefined();
    expect(calls).toBe(2);
    expect(execute).not.toHaveBeenCalled();
  });

  test("keeps a pre-intent tool failure sink error distinct from an unknown outcome", async () => {
    const execute = vi.fn(() => Promise.resolve({ content: "must not run" }));
    const tool = { ...createTool(execute), risk: "unexpected" as never };
    const append = (event: KernelEvent): Promise<void> =>
      event.type === "tool_failed"
        ? Promise.reject(new Error("raw sink failure"))
        : Promise.resolve();

    await expect(
      createPiKernelSession(createOptions(new ToolThenStopModel(), tool, append)).startTurn("run"),
    ).rejects.toMatchObject({ code: "EVENT_SINK_UNAVAILABLE" });
    expect(execute).not.toHaveBeenCalled();
  });

  test("contains listener failures outside the durability and execution paths", async () => {
    const events: KernelEvent[] = [];
    const execute = vi.fn(() => Promise.resolve({ content: "ok" }));
    const session = createPiKernelSession(
      createOptions(new ToolThenStopModel(), createTool(execute), (event) => {
        events.push(event);
        return Promise.resolve();
      }),
    );
    session.subscribe(() => {
      throw new Error("client projection disconnected");
    });

    await session.startTurn("run");

    expect(execute).toHaveBeenCalledOnce();
    expect(events.some((event) => event.type === "tool_succeeded")).toBe(true);
    expect(events.some((event) => event.type === "run_completed")).toBe(true);
  });

  test("marks a timed-out in-flight effect unknown and ignores its late completion", async () => {
    let complete: ((value: { content: string }) => void) | undefined;
    const execute = vi.fn(
      () =>
        new Promise<{ content: string }>((resolve) => {
          complete = resolve;
        }),
    );
    const events: KernelEvent[] = [];
    const session = createPiKernelSession(
      createOptions(
        new ToolThenStopModel(),
        createTool(execute),
        (event) => {
          events.push(event);
          return Promise.resolve();
        },
        { budget: { ...budget, toolTimeoutMs: 5 } },
      ),
    );

    await expect(session.startTurn("run")).rejects.toMatchObject({ code: "TOOL_OUTCOME_UNKNOWN" });
    await expect(session.startTurn("must reject")).rejects.toMatchObject({
      code: "TOOL_OUTCOME_UNKNOWN",
    });
    expect(events).toContainEqual(
      expect.objectContaining({ type: "tool_outcome_unknown", code: "TOOL_TIMEOUT" }),
    );
    expect(events).toContainEqual(
      expect.objectContaining({ type: "run_failed", code: "TOOL_OUTCOME_UNKNOWN" }),
    );
    const unknownIndex = events.findIndex((event) => event.type === "tool_outcome_unknown");
    expect(unknownIndex).toBeGreaterThanOrEqual(0);
    expect(events.slice(unknownIndex + 1).map((event) => event.type)).toEqual(["run_failed"]);
    expect(events.some((event) => event.type === "usage")).toBe(false);
    expect(events.some((event) => event.type === "turn_completed")).toBe(false);
    expect(events.some((event) => event.type === "run_completed")).toBe(false);
    expect(events.some((event) => event.type === "tool_failed")).toBe(false);
    complete?.({ content: "late success" });
    await Promise.resolve();
    await Promise.resolve();
    expect(events.some((event) => event.type === "tool_succeeded")).toBe(false);
  });

  test("records a cooperative external failure without exposing its message", async () => {
    const events: KernelEvent[] = [];
    const tool = createTool(() => Promise.reject(new Error("explicit failure")));

    await createPiKernelSession(
      createOptions(new ToolThenStopModel(), tool, (event) => {
        events.push(event);
        return Promise.resolve();
      }),
    ).startTurn("run");

    expect(events).toContainEqual(
      expect.objectContaining({ type: "tool_failed", message: "Tool execution failed" }),
    );
    expect(events.some((event) => event.type === "tool_outcome_unknown")).toBe(false);
    expect(events.some((event) => event.type === "run_completed")).toBe(true);
  });
});
