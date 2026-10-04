import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, test } from "vitest";

import { createAgentRuntime } from "../src/index.js";
import { CapturingKernelFactory, eventTypes, sequentialEventIds } from "./support.js";

describe("agent runtime lifecycle", () => {
  test("uses secure default event identifiers and timestamps", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-defaults"),
      eventStore: store,
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
      eventStore: store,
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
      eventStore: {
        currentRevision: store.currentRevision.bind(store),
        load: store.load.bind(store),
        append: async (...args: Parameters<typeof store.append>) => {
          const result = await store.append(...args);
          if (result[0]?.type === "session_opened") opened = true;
          return result;
        },
      },
      createKernelSession: (input) => {
        expect(opened).toBe(true);
        return new CapturingKernelFactory().create(input);
      },
      createEventId: sequentialEventIds(),
    });
    runtime.dispose();
    store.close();
  });

  test("rebuilds state from durable events without opening the session twice", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const firstKernel = new CapturingKernelFactory();
    const first = await createAgentRuntime({
      sessionId: asSessionId("session-recover"),
      eventStore: store,
      createKernelSession: firstKernel.create,
      createEventId: sequentialEventIds("first"),
    });
    firstKernel.startBehavior = async (_input, sink) => {
      await sink.append({ type: "run_started", sessionId: "session-recover" });
      await sink.append({ type: "text_delta", delta: "persisted answer" });
      await sink.append({ type: "run_completed", sessionId: "session-recover" });
    };
    await first.startTurn("persisted question");
    const expected = first.snapshot();
    first.dispose();
    const recovered = await createAgentRuntime({
      sessionId: asSessionId("session-recover"),
      eventStore: store,
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
});
