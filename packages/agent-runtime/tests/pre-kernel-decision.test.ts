import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, it, vi } from "vitest";

import {
  createAgentRuntime,
  type RuntimeDecisionPort,
} from "@lyonbrown/agent-runtime/internal/index";
import {
  CapturingKernelFactory,
  eventTypes,
  runtimeEventStore,
  sequentialEventIds,
} from "@lyonbrown/agent-runtime/test/support";

describe("pre-kernel decision", () => {
  it("does not start Pi when the caller aborts a pending initial decision", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    const controller = new AbortController();
    const evaluate = vi.fn<RuntimeDecisionPort["evaluate"]>(
      ({ signal }) =>
        new Promise((_resolve, reject) => {
          signal.addEventListener(
            "abort",
            () => {
              reject(new DOMException("aborted", "AbortError"));
            },
            { once: true },
          );
        }),
    );
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-decision-request-abort"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
      decision: { evaluate },
    });

    const start = runtime.startTurn("abort before Pi", { signal: controller.signal });
    await expect.poll(() => evaluate).toHaveBeenCalledOnce();
    controller.abort();
    await start;

    expect(kernel.starts).toEqual([]);
    expect(await eventTypes(store, "session-decision-request-abort")).toEqual([
      "session_opened",
      "user_message_submitted",
      "decision_fallback_recorded",
      "run_cancelled",
    ]);
    await runtime.dispose();
    store.close();
  });

  it("does not start Pi when disposal aborts a pending initial decision", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    const evaluate = vi.fn<RuntimeDecisionPort["evaluate"]>(
      ({ signal }) =>
        new Promise((_resolve, reject) => {
          signal.addEventListener(
            "abort",
            () => {
              reject(new DOMException("aborted", "AbortError"));
            },
            { once: true },
          );
        }),
    );
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-decision-dispose-initial"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
      decision: { evaluate },
    });

    const start = runtime.startTurn("dispose before Pi");
    await expect.poll(() => evaluate).toHaveBeenCalledOnce();
    await runtime.dispose();

    await expect(start).rejects.toMatchObject({ code: "RUNTIME_DISPOSED" });
    expect(kernel.starts).toEqual([]);
    expect(await eventTypes(store, "session-decision-dispose-initial")).toEqual([
      "session_opened",
      "user_message_submitted",
      "decision_fallback_recorded",
    ]);
    store.close();
  });

  it("does not start Pi when cancellation races a pending initial decision", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    let resolveDecision: (() => void) | undefined;
    const pendingDecision = new Promise<void>((resolve) => {
      resolveDecision = resolve;
    });
    const evaluate = vi.fn<RuntimeDecisionPort["evaluate"]>(async () => {
      await pendingDecision;
      return fallbackDecision();
    });
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-decision-cancel-initial"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
      decision: { evaluate },
    });

    const start = runtime.startTurn("cancel before Pi");
    await expect.poll(() => evaluate).toHaveBeenCalledOnce();
    runtime.cancel();
    resolveDecision?.();
    await start;

    expect(kernel.starts).toEqual([]);
    expect(await eventTypes(store, "session-decision-cancel-initial")).toEqual([
      "session_opened",
      "user_message_submitted",
      "decision_fallback_recorded",
      "run_cancelled",
    ]);
    await runtime.dispose();
    store.close();
  });

  it("does not start a queued follow-up when cancellation races its decision", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    kernel.startBehavior = async (_input, sink) => {
      await sink.append({ type: "run_started", sessionId: "session-decision-cancel-follow-up" });
      await sink.append({ type: "run_completed", sessionId: "session-decision-cancel-follow-up" });
    };
    let resolveFollowUpDecision: (() => void) | undefined;
    const pendingFollowUpDecision = new Promise<void>((resolve) => {
      resolveFollowUpDecision = resolve;
    });
    let decisionCalls = 0;
    const evaluate = vi.fn<RuntimeDecisionPort["evaluate"]>(async () => {
      decisionCalls += 1;
      if (decisionCalls === 2) await pendingFollowUpDecision;
      return fallbackDecision();
    });
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-decision-cancel-follow-up"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
      decision: { evaluate },
    });
    await runtime.startTurn("first");
    await expect.poll(() => runtime.snapshot().phase).toBe("completed");

    await runtime.followUp("second");
    await expect.poll(() => evaluate).toHaveBeenCalledTimes(2);
    runtime.cancel();
    resolveFollowUpDecision?.();
    await expect
      .poll(async () => eventTypes(store, "session-decision-cancel-follow-up"))
      .toContain("run_cancelled");

    expect(kernel.starts).toEqual(["first"]);
    await runtime.dispose();
    store.close();
  });

  it("persists the typed decision before starting Pi", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    const evaluate = vi.fn<RuntimeDecisionPort["evaluate"]>().mockResolvedValue({
      kind: "decided",
      questionVersion: "system-one-v1",
      providerId: "open-jev",
      modelId: "kev-0.6b",
      modelRevision: "5a827091b3d58fe4fdfc02a5a5b2ce364047d678",
      intent: "code-change",
      risk: "medium",
      requiresTools: true,
      confidence: 0.8,
      probabilities: { chat: 0.1, "code-change": 0.8, "system-operation": 0.1 },
      action: "continue_pi",
    });
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-decision"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
      decision: { evaluate },
    });

    await runtime.startTurn("change the file");

    expect(evaluate).toHaveBeenCalledOnce();
    const request = evaluate.mock.calls[0]?.[0];
    expect(request?.input).toBe("change the file");
    expect(request?.signal).toBeInstanceOf(AbortSignal);
    expect(await eventTypes(store, "session-decision")).toEqual([
      "session_opened",
      "user_message_submitted",
      "decision_recorded",
    ]);
    expect(kernel.starts).toEqual(["change the file"]);
    await runtime.dispose();
    store.close();
  });

  it("records a sanitized Pi fallback when the decision provider fails", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-decision-fallback"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
      decision: {
        evaluate: () => Promise.reject(new Error("private model path")),
      },
    });

    await runtime.startTurn("hello");

    const events = await store.load("session-decision-fallback", {
      afterRevision: 0n,
      limit: 20,
    });
    expect(events.map(({ type }) => type)).toEqual([
      "session_opened",
      "user_message_submitted",
      "decision_fallback_recorded",
    ]);
    expect(events.at(-1)?.payload).toEqual({
      type: "decision_fallback_recorded",
      sessionId: "session-decision-fallback",
      questionVersion: "system-one-v1",
      reason: "provider_unavailable",
      action: "continue_pi",
    });
    expect(kernel.starts).toEqual(["hello"]);
    await runtime.dispose();
    store.close();
  });

  it("records an aborted decision without exposing the provider failure", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const controller = new AbortController();
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-decision-aborted"),
      eventStore: runtimeEventStore(store),
      createKernelSession: new CapturingKernelFactory().create,
      createEventId: sequentialEventIds(),
      decision: {
        evaluate: () => {
          controller.abort();
          return Promise.reject(new Error("private provider failure"));
        },
      },
    });

    await runtime.startTurn("hello", { signal: controller.signal });
    const events = await store.load("session-decision-aborted", {
      afterRevision: 0n,
      limit: 20,
    });
    expect(events.map(({ type }) => type)).toEqual([
      "session_opened",
      "user_message_submitted",
      "decision_fallback_recorded",
      "run_cancelled",
    ]);
    expect(events.at(-2)?.payload).toMatchObject({
      type: "decision_fallback_recorded",
      reason: "aborted",
    });
    await runtime.dispose();
    store.close();
  });

  it("recovers a completed session containing a pre-kernel decision", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernel = new CapturingKernelFactory();
    kernel.startBehavior = async (_input, sink) => {
      await sink.append({ type: "run_started", sessionId: "session-decision-recovery" });
      await sink.append({ type: "run_completed", sessionId: "session-decision-recovery" });
    };
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-decision-recovery"),
      eventStore: runtimeEventStore(store),
      createKernelSession: kernel.create,
      createEventId: sequentialEventIds(),
      decision: {
        evaluate: () =>
          Promise.resolve({
            kind: "fallback",
            questionVersion: "system-one-v1",
            reason: "model_missing",
            action: "continue_pi",
          }),
      },
    });
    await runtime.startTurn("hello");
    await expect.poll(() => runtime.snapshot().phase).toBe("completed");
    await runtime.dispose();

    const recovered = await createAgentRuntime({
      sessionId: asSessionId("session-decision-recovery"),
      eventStore: runtimeEventStore(store),
      createKernelSession: new CapturingKernelFactory().create,
      createEventId: sequentialEventIds("recovered"),
    });
    expect(recovered.snapshot()).toMatchObject({
      revision: 5n,
      phase: "completed",
      turns: [{ userText: "hello", status: "completed" }],
    });
    await recovered.dispose();
    store.close();
  });
});

function fallbackDecision() {
  return {
    kind: "fallback" as const,
    questionVersion: "system-one-v1",
    reason: "provider_unavailable" as const,
    action: "continue_pi" as const,
  };
}
