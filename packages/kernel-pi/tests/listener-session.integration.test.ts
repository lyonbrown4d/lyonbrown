import { describe, expect, test, vi } from "vitest";

import {
  createPiKernelSession,
  type KernelModelAdapter,
  type PiKernelSessionOptions,
} from "../src/index.js";

describe("session listener isolation", () => {
  test("does not let a hung listener block a run or cancellation", async () => {
    let modelCalls = 0;
    const model: KernelModelAdapter = {
      model: { provider: "offline", id: "listener-session" },
      async *stream() {
        modelCalls += 1;
        await Promise.resolve();
        yield { type: "text_delta", delta: "done" } as const;
        yield { type: "done", reason: "stop" } as const;
      },
    };
    const options: PiKernelSessionOptions = {
      sessionId: "listener-session",
      systemPrompt: "safe",
      model,
      toolRegistry: { list: () => [], get: () => undefined, hasPermission: () => false },
      approvalGate: { request: () => Promise.resolve({ decision: "deny" }) },
      eventSink: { append: () => Promise.resolve() },
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

    await expect(session.startTurn("go")).resolves.toBeUndefined();
    expect(listener).toHaveBeenCalled();
    expect(() => {
      session.cancel();
    }).not.toThrow();
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
});
