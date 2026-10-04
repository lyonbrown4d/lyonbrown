import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, test, vi } from "vitest";

import { createAgentRuntime } from "../src/index.js";
import { eventTypes, runtimeEventStore, sequentialEventIds } from "./support.js";

describe("runtime background lifecycle", () => {
  test("does not start the kernel when disposed during the user-message append", async () => {
    const base = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-dispose-user-append");
    let appendEntered: (() => void) | undefined;
    let releaseAppend: (() => void) | undefined;
    const pendingAppend = new Promise<void>((resolve) => {
      appendEntered = resolve;
    });
    const starts: string[] = [];
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore: runtimeEventStore({
        currentRevision: base.currentRevision.bind(base),
        load: base.load.bind(base),
        append: async (...args: Parameters<typeof base.append>) => {
          if (args[2][0]?.type === "user_message_submitted") {
            appendEntered?.();
            await new Promise<void>((resolve) => {
              releaseAppend = resolve;
            });
          }
          return base.append(...args);
        },
      }),
      createEventId: sequentialEventIds(),
      createKernelSession: ({ eventSink }) => ({
        async startTurn(input) {
          starts.push(input);
          await eventSink.append({ type: "run_started", sessionId });
        },
        steer: () => undefined,
        followUp: () => undefined,
        cancel: () => undefined,
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });

    const start = runtime.startTurn("question");
    await pendingAppend;
    const dispose = runtime.dispose();
    releaseAppend?.();

    await expect(start).rejects.toMatchObject({ code: "RUNTIME_DISPOSED" });
    await dispose;
    expect(starts).toEqual([]);
    base.close();
  });

  test("does not start a follow-up when disposed during its claim append", async () => {
    const base = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-dispose-follow-up-claim");
    let claimEntered: (() => void) | undefined;
    let releaseClaim: (() => void) | undefined;
    const pendingClaim = new Promise<void>((resolve) => {
      claimEntered = resolve;
    });
    const starts: string[] = [];
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore: runtimeEventStore({
        currentRevision: base.currentRevision.bind(base),
        load: base.load.bind(base),
        append: async (...args: Parameters<typeof base.append>) => {
          if (args[2][0]?.type === "follow_up_started") {
            claimEntered?.();
            await new Promise<void>((resolve) => {
              releaseClaim = resolve;
            });
          }
          return base.append(...args);
        },
      }),
      createEventId: sequentialEventIds(),
      createKernelSession: ({ eventSink }) => ({
        async startTurn(input) {
          starts.push(input);
          await eventSink.append({ type: "run_started", sessionId });
          await eventSink.append({ type: "run_completed", sessionId });
        },
        steer: () => undefined,
        followUp: () => undefined,
        cancel: () => undefined,
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });

    await runtime.startTurn("first");
    await expect.poll(() => runtime.snapshot().phase).toBe("completed");
    await runtime.followUp("second");
    await pendingClaim;
    const dispose = runtime.dispose();
    releaseClaim?.();

    await dispose;
    expect(starts).toEqual(["first"]);
    base.close();
  });

  test("cancels active kernel work and bounds disposal waiting", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-bounded-dispose");
    const cancel = vi.fn();
    const disposeKernel = vi.fn();
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore: runtimeEventStore(store),
      createEventId: sequentialEventIds(),
      createKernelSession: ({ eventSink }) => ({
        async startTurn() {
          await eventSink.append({ type: "run_started", sessionId });
          await new Promise<void>(() => undefined);
        },
        steer: () => undefined,
        followUp: () => undefined,
        cancel,
        subscribe: () => () => undefined,
        dispose: disposeKernel,
      }),
    });

    await runtime.startTurn("question");
    await expect.poll(() => runtime.snapshot().phase).toBe("running");
    await runtime.dispose();

    expect(cancel).toHaveBeenCalledOnce();
    expect(disposeKernel).toHaveBeenCalledOnce();
    store.close();
  });

  test("fails closed when recovering a submitted turn", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-recovered-submitted");
    const events = [
      { type: "session_opened", sessionId },
      { type: "user_message_submitted", sessionId, text: "first" },
    ] as const;
    await store.append(
      sessionId,
      0n,
      events.map((event, index) => ({
        eventId: `submitted-${String(index)}`,
        type: event.type,
        payload: event,
      })),
    );
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore: runtimeEventStore(store),
      createEventId: sequentialEventIds(),
      createKernelSession: () => ({
        startTurn: () => Promise.resolve(),
        steer: () => undefined,
        followUp: () => undefined,
        cancel: () => undefined,
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });

    await expect(runtime.startTurn("second")).rejects.toMatchObject({ code: "TERMINAL_SESSION" });
    expect(await eventTypes(store, sessionId)).toEqual(events.map((event) => event.type));
    await runtime.dispose();
    store.close();
  });
});
