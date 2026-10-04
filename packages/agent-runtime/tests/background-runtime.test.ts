import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, test } from "vitest";

import { createAgentRuntime } from "../src/index.js";
import { CapturingKernelFactory, runtimeEventStore, sequentialEventIds } from "./support.js";

describe("supervised runtime commands", () => {
  test("rejects steering when no turn is active", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-idle-steering"),
      eventStore: runtimeEventStore(store),
      createKernelSession: new CapturingKernelFactory().create,
      createEventId: sequentialEventIds(),
    });

    await expect(runtime.steer("focus")).rejects.toMatchObject({ code: "TERMINAL_SESSION" });
    runtime.dispose();
    store.close();
  });

  test("returns the durable start receipt before a blocked kernel completes", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    let releaseKernel: (() => void) | undefined;
    let kernelStarted: (() => void) | undefined;
    const started = new Promise<void>((resolve) => {
      kernelStarted = resolve;
    });
    const kernel = new CapturingKernelFactory();
    kernel.startBehavior = async (_input, sink) => {
      kernelStarted?.();
      await new Promise<void>((resolve) => {
        releaseKernel = resolve;
      });
      await sink.append({ type: "run_started", sessionId: "session-background" });
      await sink.append({ type: "run_completed", sessionId: "session-background" });
    };
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-background"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
    });

    const receipt = runtime.startTurn("question");
    await started;
    await receipt;
    expect(runtime.snapshot()).toMatchObject({ revision: 2n, turns: [{ status: "submitted" }] });

    releaseKernel?.();
    await expect.poll(() => runtime.snapshot().phase).toBe("completed");
    runtime.dispose();
    store.close();
  });

  test("projects a sanitized failure when supervised kernel work rejects after its receipt", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    kernel.startBehavior = () => Promise.reject(new Error("raw model secret"));
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-background-failure"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
    });

    await expect(runtime.startTurn("question")).resolves.toBeUndefined();
    await expect.poll(() => runtime.snapshot().phase).toBe("failed");
    expect(runtime.snapshot()).toMatchObject({
      turns: [{ status: "failed" }],
      lastError: { code: "KERNEL_FAILURE", message: "Kernel session failed" },
    });
    expect(runtime.snapshot().lastError?.message).not.toContain("raw model secret");
    runtime.dispose();
    store.close();
  });

  test("atomically occupies a turn while its user message append is pending", async () => {
    const base = openEventStore({ testOnlyMemory: true });
    let release: (() => void) | undefined;
    let block = false;
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-atomic-start"),
      eventStore: runtimeEventStore({
        currentRevision: base.currentRevision.bind(base),
        load: base.load.bind(base),
        append: async (...args: Parameters<typeof base.append>) => {
          if (block) {
            await new Promise<void>((resolve) => {
              release = resolve;
            });
          }
          return base.append(...args);
        },
      }),
      createKernelSession: new CapturingKernelFactory().create,
      createEventId: sequentialEventIds(),
    });
    block = true;
    const first = runtime.startTurn("first");
    await Promise.resolve();

    await expect(runtime.startTurn("second")).rejects.toMatchObject({ code: "SESSION_BUSY" });
    release?.();
    await first;
    runtime.dispose();
    base.close();
  });

  test("persists steering and drains durable follow-ups as later kernel runs", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const starts: string[] = [];
    let releaseFirst: (() => void) | undefined;
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-durable-controls"),
      eventStore: runtimeEventStore(store),
      createEventId: sequentialEventIds(),
      createKernelSession: ({ eventSink }) => ({
        async startTurn(input) {
          starts.push(input);
          await eventSink.append({ type: "run_started", sessionId: "session-durable-controls" });
          if (input === "first") {
            await new Promise<void>((resolve) => {
              releaseFirst = resolve;
            });
          }
          await eventSink.append({ type: "turn_started", turn: 1 });
          await eventSink.append({ type: "turn_completed", turn: 1 });
          await eventSink.append({ type: "run_completed", sessionId: "session-durable-controls" });
        },
        steer: () => undefined,
        followUp: () => {
          throw new Error("runtime must own the follow-up queue");
        },
        cancel: () => undefined,
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });

    await runtime.startTurn("first");
    await expect.poll(() => starts).toEqual(["first"]);
    await runtime.steer("focus");
    await runtime.followUp("second");
    expect(runtime.snapshot()).toMatchObject({
      queuedFollowUps: ["second"],
      turns: [{ steering: ["focus"] }],
    });
    releaseFirst?.();

    await expect.poll(() => starts).toEqual(["first", "second"]);
    await expect.poll(() => runtime.snapshot().turns).toHaveLength(2);
    expect(runtime.snapshot()).toMatchObject({
      phase: "completed",
      queuedFollowUps: [],
      turns: [{ userText: "first" }, { userText: "second", status: "completed" }],
    });
    runtime.dispose();
    store.close();
  });

  test("sanitizes rejected steering and preserves sink failures for durable controls", async () => {
    const base = openEventStore({ testOnlyMemory: true });
    let rejectWrites = false;
    let release: (() => void) | undefined;
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-control-failures"),
      eventStore: runtimeEventStore({
        currentRevision: base.currentRevision.bind(base),
        load: base.load.bind(base),
        append: (...args: Parameters<typeof base.append>) =>
          rejectWrites ? Promise.reject(new Error("raw store secret")) : base.append(...args),
      }),
      createEventId: sequentialEventIds(),
      createKernelSession: () => ({
        startTurn: () =>
          new Promise<void>((resolve) => {
            release = resolve;
          }),
        steer: () => {
          throw new Error("raw steering secret");
        },
        followUp: () => undefined,
        cancel: () => undefined,
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });
    await runtime.startTurn("question");
    await expect(runtime.steer("focus")).rejects.toMatchObject({
      code: "KERNEL_FAILURE",
      message: "Kernel session failed",
    });
    rejectWrites = true;
    await expect(runtime.followUp("later")).rejects.toMatchObject({
      code: "EVENT_SINK_UNAVAILABLE",
      message: "Durable runtime event sink unavailable",
    });
    release?.();
    runtime.dispose();
    base.close();
  });
});
