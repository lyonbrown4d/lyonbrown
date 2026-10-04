import { describe, expect, test, vi } from "vitest";

import {
  createPiKernelSession,
  type KernelEvent,
  type KernelModelAdapter,
  type KernelModelRequest,
  type PiKernelSessionOptions,
} from "../src/index.js";

describe("session listener isolation", () => {
  test("does not let a hung listener block a run or cancellation", async () => {
    let modelCalls = 0;
    const durable: KernelEvent[] = [];
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "listener-session" },
      async *stream(request) {
        modelCalls += 1;
        await new Promise<void>((resolve) => {
          request.signal.addEventListener(
            "abort",
            () => {
              resolve();
            },
            { once: true },
          );
        });
        yield { type: "error", code: "aborted", message: "aborted" } as const;
      },
    };
    const options: PiKernelSessionOptions = {
      sessionId: "listener-session",
      systemPrompt: "safe",
      model,
      toolRegistry: { list: () => [], get: () => undefined, hasPermission: () => false },
      approvalGate: { request: () => Promise.resolve({ decision: "deny" }) },
      eventSink: {
        append: (event) => {
          durable.push(event);
          return Promise.resolve();
        },
      },
      secretResolver: { resolve: () => Promise.resolve("secret") },
      budget: {
        maxInputBytes: 1_024,
        maxTurns: 2,
        maxSteps: 2,
        maxOutputBytes: 1_024,
        maxToolResultBytes: 256,
        maxValueDepth: 5,
        maxValueStringBytes: 256,
        maxCollectionEntries: 32,
        maxValueNodes: 128,
        toolTimeoutMs: 100,
      },
    };
    const session = createPiKernelSession(options);
    const listener = vi.fn(() => new Promise<void>(() => undefined));
    session.subscribe(listener);

    const run = session.startTurn("go");
    await vi.waitFor(() => {
      expect(modelCalls).toBe(1);
      expect(listener).toHaveBeenCalledOnce();
    });
    expect(() => {
      session.cancel();
    }).not.toThrow();
    await expect(run).resolves.toBeUndefined();
    expect(durable.some((event) => event.type === "run_cancelled")).toBe(true);
    expect(listener).toHaveBeenCalledOnce();
    session.dispose();
    session.dispose();
    await expect(session.startTurn("disposed")).rejects.toMatchObject({
      code: "SESSION_DISPOSED",
    });
    expect(() => {
      session.steer("disposed");
    }).toThrow(expect.objectContaining({ code: "SESSION_DISPOSED" }));
    expect(modelCalls).toBe(1);
  });

  test("keeps listener delivery FIFO when cancel makes an in-flight tool outcome unknown", async () => {
    const durable: KernelEvent[] = [];
    const trace: string[] = [];
    const releases: (() => void)[] = [];
    let active = 0;
    let maxActive = 0;
    let modelCalls = 0;
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "listener-cancel-fifo" },
      async *stream(request: KernelModelRequest) {
        modelCalls += 1;
        await Promise.resolve();
        if (request.signal.aborted) {
          yield { type: "error", code: "aborted", message: "aborted" } as const;
          return;
        }
        yield {
          type: "tool_call",
          callId: "effect-1",
          name: "effect",
          arguments: { value: "safe" },
        } as const;
        yield { type: "done", reason: "tool_use" } as const;
      },
    };
    const options: PiKernelSessionOptions = {
      sessionId: "listener-cancel-fifo",
      systemPrompt: "safe",
      model,
      toolRegistry: {
        list: () => [
          {
            name: "effect",
            label: "Effect",
            description: "Run one effect",
            inputSchema: {
              type: "object",
              properties: { value: { type: "string" } },
              required: ["value"],
              additionalProperties: false,
            },
            permissions: ["effect.run"],
            risk: "low",
            execute: () => new Promise(() => undefined),
          },
        ],
        get: (name) => (name === "effect" ? options.toolRegistry.list()[0] : undefined),
        hasPermission: () => true,
      },
      approvalGate: { request: () => Promise.resolve({ decision: "approve" }) },
      eventSink: {
        append: (event) => {
          durable.push(event);
          return Promise.resolve();
        },
      },
      secretResolver: { resolve: () => Promise.resolve("secret") },
      budget: {
        maxInputBytes: 1_024,
        maxTurns: 2,
        maxSteps: 2,
        maxOutputBytes: 1_024,
        maxToolResultBytes: 256,
        maxValueDepth: 5,
        maxValueStringBytes: 256,
        maxCollectionEntries: 32,
        maxValueNodes: 128,
        toolTimeoutMs: 10_000,
      },
    };
    const session = createPiKernelSession(options);
    session.subscribe(
      (event) =>
        new Promise<void>((resolve) => {
          active += 1;
          maxActive = Math.max(maxActive, active);
          trace.push(`start:${event.type}`);
          releases.push(() => {
            trace.push(`end:${event.type}`);
            active -= 1;
            resolve();
          });
        }),
    );

    const run = session.startTurn("go");
    await vi.waitFor(() => {
      expect(durable.some((event) => event.type === "tool_started")).toBe(true);
      expect(trace).toEqual(["start:run_started"]);
    });
    session.cancel();
    await expect(run).rejects.toMatchObject({ code: "TOOL_OUTCOME_UNKNOWN" });

    expect(durable.map((event) => event.type)).toEqual([
      "run_started",
      "turn_started",
      "tool_started",
      "tool_outcome_unknown",
      "run_failed",
    ]);
    expect(maxActive).toBe(1);
    for (const type of durable.map((event) => event.type)) {
      await vi.waitFor(() => {
        expect(trace.at(-1)).toBe(`start:${type}`);
      });
      releases.shift()?.();
      await vi.waitFor(() => {
        expect(trace.at(-1)).toBe(`end:${type}`);
      });
    }
    expect(trace).toEqual(durable.flatMap((event) => [`start:${event.type}`, `end:${event.type}`]));
    expect(maxActive).toBe(1);
    expect(modelCalls).toBe(1);
    session.dispose();
  });
});
