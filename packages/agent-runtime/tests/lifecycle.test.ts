import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, test, vi } from "vitest";

import { createAgentRuntime } from "../src/index.js";
import {
  CapturingKernelFactory,
  eventTypes,
  runtimeEventStore,
  sequentialEventIds,
} from "./support.js";

describe("agent runtime lifecycle", () => {
  test("uses secure default event identifiers and timestamps", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-defaults"),
      eventStore: runtimeEventStore(store),
      createKernelSession: new CapturingKernelFactory().create,
    });
    const [opened] = await store.load("session-defaults", { afterRevision: 0n, limit: 1 });
    expect(opened?.eventId).toMatch(/^[0-9a-f-]{36}$/u);
    expect(Number.isNaN(Date.parse(opened?.occurredAt ?? ""))).toBe(false);
    runtime.dispose();
    store.close();
  });

  test("durably opens an empty session before exposing its initial snapshot", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-open"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
    });
    expect(runtime.snapshot()).toMatchObject({ phase: "ready", revision: 1n });
    expect(await eventTypes(store, "session-open")).toEqual(["session_opened"]);
    runtime.dispose();
    expect(store.diagnostics().schemaVersion).toBeGreaterThan(0);
    store.close();
  });

  test("persists SessionOpened before constructing the kernel session", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    let opened = false;
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-before-kernel"),
      eventStore: runtimeEventStore({
        currentRevision: store.currentRevision.bind(store),
        load: store.load.bind(store),
        append: async (...args: Parameters<typeof store.append>) => {
          const result = await store.append(...args);
          if (result[0]?.type === "session_opened") opened = true;
          return result;
        },
      }),
      createKernelSession: (input) => {
        expect(opened).toBe(true);
        return new CapturingKernelFactory().create(input);
      },
      createEventId: sequentialEventIds(),
    });
    runtime.dispose();
    store.close();
  });

  test("sanitizes a kernel factory failure after opening the session", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    await expect(
      createAgentRuntime({
        sessionId: asSessionId("session-kernel-construction-failure"),
        eventStore: runtimeEventStore(store),
        createKernelSession: () => {
          throw new Error("raw constructor secret");
        },
      }),
    ).rejects.toMatchObject({ code: "KERNEL_FAILURE", message: "Kernel session failed" });
    expect(await eventTypes(store, "session-kernel-construction-failure")).toEqual([
      "session_opened",
    ]);
    store.close();
  });

  test("rebuilds state from durable events without opening the session twice", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const firstKernel = new CapturingKernelFactory();
    const first = await createAgentRuntime({
      sessionId: asSessionId("session-recover"),
      eventStore: runtimeEventStore(store),
      createKernelSession: firstKernel.create,
      createEventId: sequentialEventIds("first"),
    });
    firstKernel.startBehavior = async (_input, sink) => {
      await sink.append({ type: "run_started", sessionId: "session-recover" });
      await sink.append({ type: "text_delta", delta: "persisted answer" });
      await sink.append({ type: "run_completed", sessionId: "session-recover" });
    };
    await first.startTurn("persisted question");
    await expect.poll(() => first.snapshot().phase).toBe("completed");
    const expected = first.snapshot();
    first.dispose();
    const recovered = await createAgentRuntime({
      sessionId: asSessionId("session-recover"),
      eventStore: runtimeEventStore(store),
      createKernelSession: new CapturingKernelFactory().create,
      createEventId: sequentialEventIds("second"),
    });
    expect(recovered.snapshot()).toEqual(expected);
    expect((await eventTypes(store, "session-recover"))[0]).toBe("session_opened");
    expect(
      (await eventTypes(store, "session-recover")).filter((type) => type === "session_opened"),
    ).toHaveLength(1);
    recovered.dispose();
    store.close();
  });

  test("recovers a completed run's durable follow-up and starts it as the next turn", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-follow-up-recovery");
    const events = [
      { type: "session_opened", sessionId },
      { type: "user_message_submitted", sessionId, text: "first" },
      { type: "run_started", sessionId },
      { type: "steering_submitted", sessionId, text: "focus" },
      { type: "run_completed", sessionId },
      { type: "follow_up_queued", sessionId, text: "second" },
    ] as const;
    await store.append(
      sessionId,
      0n,
      events.map((event, index) => ({
        eventId: `follow-up-${String(index)}`,
        type: event.type,
        payload: event,
      })),
    );
    const starts: string[] = [];
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore: runtimeEventStore(store),
      createEventId: sequentialEventIds("recovered"),
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

    await expect.poll(() => starts).toEqual(["second"]);
    await expect.poll(() => runtime.snapshot().phase).toBe("completed");
    expect(runtime.snapshot()).toMatchObject({
      queuedFollowUps: [],
      turns: [
        { userText: "first", steering: ["focus"] },
        { userText: "second", status: "completed" },
      ],
    });
    expect(await eventTypes(store, sessionId)).toEqual([
      ...events.map((event) => event.type),
      "follow_up_started",
      "run_started",
      "run_completed",
    ]);
    runtime.dispose();
    store.close();
  });

  test("rejects a new turn after recovering an unresolved running tool", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-unresolved-recovery");
    const events = [
      { type: "session_opened", sessionId },
      { type: "user_message_submitted", sessionId, text: "change it" },
      { type: "run_started", sessionId },
      {
        type: "tool_started",
        sessionId,
        callId: "call-pending",
        toolName: "write_file",
        argumentsPreview: { path: "README.md" },
      },
    ] as const;
    await store.append(
      sessionId,
      0n,
      events.map((event, index) => ({
        eventId: `recovery-${String(index)}`,
        type: event.type,
        payload: event,
      })),
    );
    const kernel = new CapturingKernelFactory();
    const replacementCancel = vi.fn();
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore: runtimeEventStore(store),
      createKernelSession: (input) => ({
        ...kernel.create(input),
        cancel: replacementCancel,
      }),
      createEventId: sequentialEventIds("new"),
    });

    await expect(runtime.startTurn("must not clear pending tool")).rejects.toMatchObject({
      code: "TERMINAL_SESSION",
    });
    expect(() => {
      runtime.cancel();
    }).toThrow(expect.objectContaining({ code: "TERMINAL_SESSION" }));
    expect(kernel.starts).toEqual([]);
    expect(replacementCancel).not.toHaveBeenCalled();
    expect(await eventTypes(store, sessionId)).toEqual(events.map((event) => event.type));
    runtime.dispose();
    store.close();
  });
});
