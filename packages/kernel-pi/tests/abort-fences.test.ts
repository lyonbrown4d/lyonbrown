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
} from "../src/index.js";

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
  readonly model = { provider: "offline", id: "abort-fences" };
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
): PiKernelSessionOptions {
  return {
    sessionId: "abort-session",
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
  };
}

describe("abort fences", () => {
  test("does not enter Pi when the caller signal is already aborted", async () => {
    const controller = new AbortController();
    controller.abort();
    const model = new ToolThenStopModel();
    const events: KernelEvent[] = [];
    const session = createPiKernelSession(
      createOptions(
        model,
        createTool(() => Promise.resolve({ content: "ok" })),
        (event) => {
          events.push(event);
          return Promise.resolve();
        },
      ),
    );

    await expect(session.startTurn("run", { signal: controller.signal })).rejects.toHaveProperty(
      "name",
      "AbortError",
    );
    expect(model.calls).toBe(0);
    expect(events).toEqual([]);
  });

  test("rechecks abort after ToolStarted persistence and never calls execute", async () => {
    const controller = new AbortController();
    const events: KernelEvent[] = [];
    const execute = vi.fn(() => Promise.resolve({ content: "must not run" }));
    const session = createPiKernelSession(
      createOptions(new ToolThenStopModel(), createTool(execute), (event) => {
        events.push(event);
        if (event.type === "tool_started") controller.abort();
        return Promise.resolve();
      }),
    );

    await expect(session.startTurn("run", { signal: controller.signal })).rejects.toMatchObject({
      code: "TOOL_OUTCOME_UNKNOWN",
    });
    expect(execute).not.toHaveBeenCalled();
    expect(events.some((event) => event.type === "tool_outcome_unknown")).toBe(true);
  });

  test("rechecks abort after approval and secret resolution", async () => {
    for (const stage of ["approval", "secret"] as const) {
      const controller = new AbortController();
      const events: KernelEvent[] = [];
      const execute = vi.fn(() => Promise.resolve({ content: "must not run" }));
      const tool: KernelToolDefinition = {
        ...createTool(execute),
        risk: stage === "approval" ? "high" : "low",
      };
      const model: KernelModelAdapter = {
        model: { provider: "offline", id: stage },
        async *stream() {
          await Promise.resolve();
          yield {
            type: "tool_call",
            callId: "call-1",
            name: "effect",
            arguments: {
              value: stage === "secret" ? { kind: "secretRef", reference: "ref" } : "safe",
            },
          };
          yield { type: "done", reason: "tool_use" };
        },
      };
      const options = createOptions(model, tool, (event) => {
        events.push(event);
        return Promise.resolve();
      });
      options.approvalGate.request = () => {
        if (stage === "approval") controller.abort();
        return Promise.resolve({ decision: "approve" });
      };
      options.secretResolver.resolve = () => {
        controller.abort();
        return Promise.resolve("resolved");
      };

      const run = createPiKernelSession(options).startTurn("run", { signal: controller.signal });
      if (stage === "approval") {
        await expect(run).resolves.toBeUndefined();
        expect(events.some((event) => event.type === "tool_started")).toBe(false);
      } else {
        await expect(run).rejects.toMatchObject({ code: "TOOL_OUTCOME_UNKNOWN" });
        expect(events.some((event) => event.type === "tool_outcome_unknown")).toBe(true);
      }
      expect(execute).not.toHaveBeenCalled();
    }
  });
});
