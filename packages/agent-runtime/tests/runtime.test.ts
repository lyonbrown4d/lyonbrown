import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, test } from "vitest";

import { createAgentRuntime } from "../src/index.js";
import { CapturingKernelFactory, runtimeEventStore, sequentialEventIds } from "./support.js";

describe("agent runtime", () => {
  test("publishes only after append commits and keeps only the latest pending snapshot", async () => {
    const base = openEventStore({ testOnlyMemory: true });
    let release: (() => void) | undefined;
    let gateNext = false;
    const eventStore = {
      currentRevision: base.currentRevision.bind(base),
      load: base.load.bind(base),
      append: async (...args: Parameters<typeof base.append>) => {
        if (gateNext) {
          await new Promise<void>((resolve) => {
            release = resolve;
          });
        }
        return base.append(...args);
      },
    };
    const kernel = new CapturingKernelFactory();
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-feed"),
      eventStore: runtimeEventStore(eventStore),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
    });
    const feed = runtime.snapshots();
    await expect(feed.next()).resolves.toMatchObject({ value: { revision: 1n } });

    gateNext = true;
    const pending = kernel.emit({ type: "run_started", sessionId: "session-feed" });
    await Promise.resolve();
    expect(runtime.snapshot().revision).toBe(1n);
    release?.();
    await pending;
    gateNext = false;
    await kernel.emit({ type: "text_delta", delta: "a" });
    await kernel.emit({ type: "text_delta", delta: "b" });

    await expect(feed.next()).resolves.toMatchObject({
      value: { revision: 4n },
      done: false,
    });
    feed.close();
    runtime.dispose();
    base.close();
  });

  test("serializes concurrent event writes for one session", async () => {
    const base = openEventStore({ testOnlyMemory: true });
    let active = 0;
    let maximumActive = 0;
    const eventStore = {
      currentRevision: base.currentRevision.bind(base),
      load: base.load.bind(base),
      append: async (...args: Parameters<typeof base.append>) => {
        active += 1;
        maximumActive = Math.max(maximumActive, active);
        await Promise.resolve();
        const result = await base.append(...args);
        active -= 1;
        return result;
      },
    };
    const kernel = new CapturingKernelFactory();
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-serial"),
      eventStore: runtimeEventStore(eventStore),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
    });

    await Promise.all([
      kernel.emit({ type: "run_started", sessionId: "session-serial" }),
      kernel.emit({ type: "text_delta", delta: "one" }),
      kernel.emit({
        type: "usage",
        usage: { inputTokens: 3, outputTokens: 2, totalTokens: 5 },
      }),
    ]);

    expect(maximumActive).toBe(1);
    expect(runtime.snapshot()).toMatchObject({
      revision: 4n,
      usage: { inputTokens: 3, outputTokens: 2, totalTokens: 5 },
    });
    runtime.dispose();
    base.close();
  });

  test("makes a sink failure sticky without publishing an uncommitted snapshot", async () => {
    const base = openEventStore({ testOnlyMemory: true });
    let rejectWrites = false;
    const eventStore = {
      currentRevision: base.currentRevision.bind(base),
      load: base.load.bind(base),
      append: (...args: Parameters<typeof base.append>) =>
        rejectWrites ? Promise.reject(new Error("database secret")) : base.append(...args),
    };
    const kernel = new CapturingKernelFactory();
    kernel.startBehavior = async (_input, sink) => {
      await sink.append({ type: "run_started", sessionId: "session-fatal" });
    };
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-fatal"),
      eventStore: runtimeEventStore(eventStore),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
    });
    rejectWrites = true;

    await expect(runtime.startTurn("question")).rejects.toMatchObject({
      code: "EVENT_SINK_UNAVAILABLE",
      message: "Durable runtime event sink unavailable",
    });
    expect(runtime.snapshot().revision).toBe(1n);
    await expect(runtime.startTurn("must reject")).rejects.toMatchObject({
      code: "EVENT_SINK_UNAVAILABLE",
    });
    expect(kernel.starts).toEqual([]);
    runtime.dispose();
    base.close();
  });

  test("keeps an uncertain tool outcome terminal after it is durable", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    kernel.startBehavior = async (_input, sink) => {
      await sink.append({ type: "run_started", sessionId: "session-unknown" });
      await sink.append({
        type: "tool_outcome_unknown",
        callId: "call-1",
        toolName: "write_file",
        code: "TOOL_TIMEOUT",
        message: "Tool outcome is unknown",
      });
      await sink.append({
        type: "run_failed",
        sessionId: "session-unknown",
        code: "TOOL_OUTCOME_UNKNOWN",
        message: "Tool outcome is unknown",
      });
    };
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-unknown"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
    });

    await runtime.startTurn("change it");
    expect(runtime.snapshot()).toMatchObject({
      phase: "failed_terminal",
      outcomeUnknown: true,
    });
    await expect(runtime.startTurn("must reject")).rejects.toMatchObject({
      code: "TERMINAL_SESSION",
    });
    runtime.dispose();
    store.close();
  });

  test("closes snapshot feeds and kernel session on dispose", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-dispose"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
    });
    const feed = runtime.snapshots();
    await feed.next();
    runtime.dispose();

    await expect(feed.next()).resolves.toEqual({ value: undefined, done: true });
    await expect(runtime.startTurn("no")).rejects.toMatchObject({ code: "RUNTIME_DISPOSED" });
    expect(() => {
      runtime.steer("no");
    }).toThrow(expect.objectContaining({ code: "RUNTIME_DISPOSED" }));
    store.close();
  });

  test("closes feeds even when kernel disposal fails", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-dispose-failure"),
      eventStore: runtimeEventStore(store),
      createEventId: sequentialEventIds(),
      createKernelSession: () => ({
        startTurn: () => Promise.resolve(),
        steer: () => undefined,
        followUp: () => undefined,
        cancel: () => undefined,
        subscribe: () => () => undefined,
        dispose: () => {
          throw new Error("raw kernel secret");
        },
      }),
    });
    const feed = runtime.snapshots();
    await feed.next();
    expect(() => {
      runtime.dispose();
    }).toThrow(
      expect.objectContaining({ code: "KERNEL_FAILURE", message: "Kernel session failed" }),
    );
    await expect(feed.next()).resolves.toEqual({ value: undefined, done: true });
    expect(() => {
      runtime.dispose();
    }).not.toThrow();
    store.close();
  });

  test("bounds turn concurrency and delegates live controls", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const calls: string[] = [];
    let release: (() => void) | undefined;
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-controls"),
      eventStore: runtimeEventStore(store),
      createEventId: sequentialEventIds(),
      createKernelSession: () => ({
        startTurn: () =>
          new Promise<void>((resolve) => {
            release = resolve;
          }),
        steer: (input) => {
          calls.push(`steer:${input}`);
        },
        followUp: (input) => {
          calls.push(`follow:${input}`);
        },
        cancel: () => {
          calls.push("cancel");
        },
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });
    const active = runtime.startTurn("first");
    await Promise.resolve();

    await expect(runtime.startTurn("second")).rejects.toMatchObject({ code: "SESSION_BUSY" });
    runtime.steer("focus");
    runtime.followUp("then summarize");
    runtime.cancel();
    expect(calls).toEqual(["steer:focus", "follow:then summarize", "cancel"]);
    release?.();
    await active;

    const aborted = new AbortController();
    aborted.abort();
    await expect(runtime.startTurn("aborted", { signal: aborted.signal })).rejects.toMatchObject({
      name: "AbortError",
    });
    runtime.dispose();
    store.close();
  });
});
