import { describe, expect, test, vi } from "vitest";

import {
  createPiKernelSession,
  type KernelEvent,
  type KernelModelAdapter,
  type KernelToolDefinition,
  type PiKernelSessionOptions,
} from "../src/index.js";

describe("secret pattern budgets", () => {
  test("fails with resource exhaustion before tool execution", async () => {
    const events: KernelEvent[] = [];
    const execute = vi.fn(() => Promise.resolve({ content: "must not run" }));
    const tool: KernelToolDefinition = {
      name: "effect",
      label: "Effect",
      description: "Bounded effect",
      inputSchema: { type: "object", additionalProperties: { type: "string" } },
      permissions: [],
      risk: "low",
      execute,
    };
    let modelCalls = 0;
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "secret-budget" },
      async *stream() {
        await Promise.resolve();
        modelCalls += 1;
        if (modelCalls > 1) {
          yield { type: "done", reason: "stop" } as const;
          return;
        }
        yield {
          type: "tool_call",
          callId: "call-1",
          name: "effect",
          arguments: {
            first: { kind: "secretRef", reference: "one" },
            second: { kind: "secretRef", reference: "two" },
          },
        } as const;
        yield { type: "done", reason: "tool_use" } as const;
      },
    };
    const options: PiKernelSessionOptions = {
      sessionId: "secret-budget",
      systemPrompt: "safe",
      model,
      toolRegistry: { list: () => [tool], get: () => tool, hasPermission: () => true },
      approvalGate: { request: () => Promise.resolve({ decision: "approve" }) },
      eventSink: {
        append: (event) => {
          events.push(event);
          return Promise.resolve();
        },
      },
      secretResolver: { resolve: (reference) => Promise.resolve(reference) },
      budget: {
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
        maxSecretCount: 1,
        maxSecretBytes: 64,
      },
    };

    await createPiKernelSession(options).startTurn("run");

    expect(execute).not.toHaveBeenCalled();
    expect(events).toContainEqual(
      expect.objectContaining({ type: "tool_failed", code: "INPUT_BUDGET_EXCEEDED" }),
    );
  });
});
