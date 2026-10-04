import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, test } from "vitest";

import { createAgentRuntime } from "../src/index.js";
import {
  CapturingKernelFactory,
  eventTypes,
  runtimeEventStore,
  sequentialEventIds,
} from "./support.js";

describe("runtime event writer races", () => {
  test("rejects late steering without poisoning a concurrent durable follow-up", async () => {
    const base = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-control-race");
    let releaseFirst: (() => void) | undefined;
    let releaseCompletionAppend: (() => void) | undefined;
    let completionAppendEntered: (() => void) | undefined;
    const completionPending = new Promise<void>((resolve) => {
      completionAppendEntered = resolve;
    });
    const starts: string[] = [];
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore: runtimeEventStore({
        currentRevision: base.currentRevision.bind(base),
        load: base.load.bind(base),
        append: async (...args: Parameters<typeof base.append>) => {
          if (args[2][0]?.type === "run_completed" && starts.length === 1) {
            completionAppendEntered?.();
            await new Promise<void>((resolve) => {
              releaseCompletionAppend = resolve;
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
          if (starts.length === 1) {
            await new Promise<void>((resolve) => {
              releaseFirst = resolve;
            });
          }
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
    await expect.poll(() => runtime.snapshot().phase).toBe("running");
    releaseFirst?.();
    await completionPending;
    const lateSteering = runtime.steer("too late");
    const followUp = runtime.followUp("second");
    releaseCompletionAppend?.();

    await expect(lateSteering).rejects.toMatchObject({ code: "KERNEL_FAILURE" });
    await expect(followUp).resolves.toBeUndefined();
    await expect.poll(() => starts).toEqual(["first", "second"]);
    await expect.poll(() => runtime.snapshot().turns).toHaveLength(2);
    expect(await eventTypes(base, sessionId)).not.toContain("steering_submitted");
    expect(await eventTypes(base, sessionId)).toEqual(
      expect.arrayContaining(["follow_up_queued", "follow_up_started"]),
    );
    await runtime.dispose();

    const recovered = await createAgentRuntime({
      sessionId,
      eventStore: runtimeEventStore(base),
      createEventId: sequentialEventIds("recovered"),
      createKernelSession: new CapturingKernelFactory().create,
    });
    expect(recovered.snapshot()).toMatchObject({ phase: "completed", queuedFollowUps: [] });
    await recovered.dispose();
    base.close();
  });

  test("rejects a tool event after completion without appending or poisoning the writer", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-late-tool");
    const kernel = new CapturingKernelFactory();
    kernel.startBehavior = async (_input, sink) => {
      await sink.append({ type: "run_started", sessionId });
      await sink.append({ type: "run_completed", sessionId });
    };
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
    });

    await runtime.startTurn("first");
    await expect.poll(() => runtime.snapshot().phase).toBe("completed");
    await expect(
      kernel.emit({
        type: "tool_started",
        callId: "late-call",
        toolName: "write_file",
        argumentsPreview: {},
      }),
    ).rejects.toThrow();
    expect(await eventTypes(store, sessionId)).not.toContain("tool_started");

    await runtime.startTurn("second");
    await expect.poll(() => runtime.snapshot().turns).toHaveLength(2);
    await runtime.dispose();
    store.close();
  });
});
