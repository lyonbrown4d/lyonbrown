import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, test } from "vitest";

import { createAgentRuntime } from "@lyonbrown/agent-runtime/internal/index";
import {
  eventTypes,
  runtimeEventStore,
  sequentialEventIds,
} from "@lyonbrown/agent-runtime/test/support";

describe("runtime command linearization", () => {
  test("does not launch a turn cancelled while its user message is being persisted", async () => {
    const base = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-cancel-pending-start");
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
          await eventSink.append({ type: "run_completed", sessionId });
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
    runtime.cancel();
    releaseAppend?.();

    await expect(start).resolves.toBeUndefined();
    await expect.poll(() => runtime.snapshot().phase).toBe("cancelled");
    expect(starts).toEqual([]);
    expect(await eventTypes(base, sessionId)).toEqual([
      "session_opened",
      "user_message_submitted",
      "run_cancelled",
    ]);
    await runtime.dispose();
    base.close();
  });

  test("starts a durable idle follow-up before a concurrently requested new turn", async () => {
    const base = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-follow-up-before-start");
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
          if (args[2][0]?.type === "follow_up_queued") {
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
          await eventSink.append({ type: "run_completed", sessionId });
        },
        steer: () => undefined,
        followUp: () => undefined,
        cancel: () => undefined,
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });

    const followUp = runtime.followUp("queued-first");
    await pendingAppend;
    const start = runtime.startTurn("started-second");
    releaseAppend?.();

    await expect(start).rejects.toMatchObject({ code: "SESSION_BUSY" });
    await expect(followUp).resolves.toBeUndefined();
    await expect.poll(() => starts).toEqual(["queued-first"]);
    await expect.poll(() => runtime.snapshot().phase).toBe("completed");
    expect(runtime.snapshot().turns.map((turn) => turn.userText)).toEqual(["queued-first"]);
    await runtime.dispose();
    base.close();
  });
});
