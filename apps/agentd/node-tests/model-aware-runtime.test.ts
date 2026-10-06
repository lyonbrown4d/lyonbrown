import { openEventStore } from "@lyonbrown/event-store";
import { AgentRuntimeError, type AgentRuntime } from "@lyonbrown/agent-runtime";
import {
  DEFAULT_LOCAL_MODEL,
  type LocalModelLease,
  type LocalModelLoadResult,
  type LocalModelService,
  type LocalModelStatus,
} from "@lyonbrown/local-model";
import {
  ModelSetupAction,
  ModelSetupPhase,
  RuntimePhase,
  type RuntimeSnapshot,
} from "@lyonbrown/protocol";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createModelAwareRuntime } from "../node-src/model-aware-runtime.ts";
import { createProductionRuntime } from "../node-src/production-kernel.ts";
import {
  MAX_QUEUED_FOLLOW_UPS,
  MAX_SESSION_TURNS,
  MAX_SESSION_USER_BYTES,
} from "../node-src/runtime-limits.ts";
import { FakeAgentRuntime, stateFrom } from "./runtime-adapter-support.ts";

const SESSION_ID = "a".repeat(64);

afterEach(() => {
  vi.useRealTimers();
});

function deferred<T>(): { readonly promise: Promise<T>; readonly resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

class FakeModelService implements LocalModelService {
  readonly startDownload = vi.fn(() => ({ kind: "started", operationId: "download-1" }) as const);
  readonly cancelDownload = vi.fn(() => ({ kind: "cancelled" }) as const);
  readonly dispose = vi.fn(async () => undefined);
  readonly load: () => Promise<LocalModelLoadResult>;
  private readonly listeners = new Set<(status: LocalModelStatus) => void>();

  constructor(
    private current: LocalModelStatus,
    lease: LocalModelLease,
  ) {
    this.load = vi.fn(
      async (): Promise<LocalModelLoadResult> => ({
        kind: "ready",
        model: DEFAULT_LOCAL_MODEL,
        lease,
      }),
    );
  }

  status(): LocalModelStatus {
    return this.current;
  }

  subscribe(listener: (status: LocalModelStatus) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  publish(status: LocalModelStatus): void {
    this.current = status;
    for (const listener of this.listeners) listener(status);
  }
}

function status(phase: LocalModelStatus["phase"]): LocalModelStatus {
  return {
    model: DEFAULT_LOCAL_MODEL,
    phase,
    operationId: null,
    downloadedBytes: 0,
    totalBytes: null,
    failure: null,
    selectionReason: phase === "checking" ? null : "hardware-compatible",
    allowedActions: phase === "setup-required" ? ["download"] : [],
  };
}

function lease(): LocalModelLease {
  return {
    async *stream() {
      yield { type: "text_delta", text: "Hello from local." } as const;
      yield { type: "usage", inputTokens: 4, outputTokens: 3 } as const;
      yield { type: "done", reason: "stop" } as const;
    },
    dispose: vi.fn(async () => undefined),
  };
}

function completedState(userTexts: readonly string[]) {
  const base = stateFrom([{ type: "session_opened" }], SESSION_ID);
  return {
    ...base,
    phase: "completed" as const,
    turns: userTexts.map((userText, index) => ({
      userText,
      assistantText: "",
      status: "completed" as const,
      turn: index + 1,
      steering: [],
      activities: [],
    })),
  };
}

async function waitFor(
  runtime: ReturnType<typeof createModelAwareRuntime>,
  predicate: (phase: RuntimePhase) => boolean,
): Promise<void> {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (predicate((await runtime.host.snapshot()).phase)) return;
    await new Promise<void>((resolve) => setTimeout(resolve, 5));
  }
  throw new Error("runtime state did not converge");
}

async function waitForSnapshot(
  runtime: ReturnType<typeof createModelAwareRuntime>,
  predicate: (snapshot: RuntimeSnapshot) => boolean,
): Promise<RuntimeSnapshot> {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const snapshot = await runtime.host.snapshot();
    if (predicate(snapshot)) return snapshot;
    await new Promise<void>((resolve) => setTimeout(resolve, 5));
  }
  throw new Error("runtime snapshot did not converge");
}

describe("model-aware production runtime", () => {
  it("does not auto-download and exposes setup-required before activation", async () => {
    const model = new FakeModelService(status("setup-required"), lease());
    const store = openEventStore({ testOnlyMemory: true });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: store,
    });

    expect(await runtime.host.snapshot()).toMatchObject({
      phase: RuntimePhase.RUNTIME_PHASE_MODEL_SETUP_REQUIRED,
      modelSetup: { managedModel: { id: DEFAULT_LOCAL_MODEL.id } },
    });
    expect(model.startDownload).not.toHaveBeenCalled();
    await expect(runtime.host.execute({ kind: "start-turn", text: "hello" })).resolves.toEqual({
      ok: false,
      error: { code: "MODEL_SETUP_REQUIRED" },
    });
    await runtime.close();
  });

  it("activates real Pi, streams local output, and persists domain events", async () => {
    const model = new FakeModelService(status("ready"), lease());
    const store = openEventStore({ testOnlyMemory: true });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: store,
    });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_READY);

    await expect(runtime.host.execute({ kind: "start-turn", text: "hello" })).resolves.toEqual({
      ok: true,
    });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_COMPLETED);
    const snapshot = await runtime.host.snapshot();
    expect(snapshot.turns.at(-1)).toMatchObject({
      userText: "hello",
      assistantText: "Hello from local.",
    });
    expect(await store.currentRevision(SESSION_ID)).toBeGreaterThan(1n);
    await runtime.close();
  });

  it("uses a daemon revision for model changes and catches up subscribers", async () => {
    const model = new FakeModelService(status("setup-required"), lease());
    const store = openEventStore({ testOnlyMemory: true });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: store,
    });
    const first = (await runtime.host.snapshot()).revision;
    const controller = new AbortController();
    const iterator = runtime.host.subscribe(first, controller.signal)[Symbol.asyncIterator]();

    model.publish(status("downloading"));
    const event = await iterator.next();
    if (event.done) throw new Error("subscription ended before catch-up");
    expect(event.value.revision).toBeGreaterThan(first);
    expect(event.value.snapshot?.revision).toBe(event.value.revision);
    controller.abort();
    await iterator.return?.();
    await runtime.close();
  });

  it("retries a failed activation explicitly without starting a download", async () => {
    const model = new FakeModelService(status("ready"), lease());
    const store = openEventStore({ testOnlyMemory: true });
    let attempts = 0;
    const createRuntime = vi.fn(async (...args: Parameters<typeof createProductionRuntime>) => {
      attempts += 1;
      if (attempts === 1) throw new Error("private activation failure");
      return createProductionRuntime(...args);
    });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: store,
      createRuntime,
    });
    const failed = await waitForSnapshot(
      runtime,
      (snapshot) => snapshot.modelSetup?.phase === ModelSetupPhase.MODEL_SETUP_PHASE_FAILED,
    );
    expect(failed.modelSetup).toMatchObject({
      failure: { message: "local model activation failed", retryable: true },
      allowedActions: [ModelSetupAction.MODEL_SETUP_ACTION_RETRY],
    });

    await expect(
      runtime.host.execute({ kind: "start-model-download", modelId: DEFAULT_LOCAL_MODEL.id }),
    ).resolves.toEqual({ ok: true });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_READY);
    expect(createRuntime).toHaveBeenCalledTimes(2);
    expect(model.startDownload).not.toHaveBeenCalled();
    await runtime.close();
  });

  it("validates subscription revisions and filters events below a client high-water mark", async () => {
    const model = new FakeModelService(status("setup-required"), lease());
    const store = openEventStore({ testOnlyMemory: true });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: store,
    });

    const invalidSubscription = runtime.host.subscribe(-1n, new AbortController().signal);
    const invalid = invalidSubscription[Symbol.asyncIterator]();
    await expect(invalid.next()).rejects.toThrow("runtime snapshot cannot be projected");

    const controller = new AbortController();
    const future = runtime.host.subscribe(100n, controller.signal)[Symbol.asyncIterator]();
    const pending = future.next();
    model.publish(status("downloading"));
    await new Promise<void>((resolve) => setTimeout(resolve, 10));
    controller.abort();
    await expect(pending).resolves.toEqual({ done: true, value: undefined });
    await runtime.close();
  });

  it("continues cleanup after a runtime or lease disposal failure", async () => {
    const modelLease = lease();
    modelLease.dispose = vi.fn(async () => Promise.reject(new Error("lease failed")));
    const model = new FakeModelService(status("ready"), modelLease);
    const store = openEventStore({ testOnlyMemory: true });
    const close = vi.spyOn(store, "close");
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: store,
    });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_READY);

    await expect(runtime.close()).rejects.toBeInstanceOf(AggregateError);
    expect(model.dispose).toHaveBeenCalledOnce();
    expect(close).toHaveBeenCalledOnce();
    await expect(runtime.close()).rejects.toBeInstanceOf(AggregateError);
  });

  it("bounds shutdown while model loading and disposes a late lease once", async () => {
    vi.useFakeTimers();
    const lateLease = lease();
    const lateLeaseDispose = vi.spyOn(lateLease, "dispose");
    const load = deferred<LocalModelLoadResult>();
    const model = new FakeModelService(status("ready"), lateLease);
    vi.mocked(model.load).mockImplementation(() => load.promise);
    const store = openEventStore({ testOnlyMemory: true });
    const closeStore = vi.spyOn(store, "close");
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: store,
    });

    let settled = false;
    const outcome = runtime.close().then(
      () => undefined,
      (error: unknown) => error,
    );
    void outcome.then(() => {
      settled = true;
    });
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(5_000);
    const settledAtDeadline = settled;
    load.resolve({ kind: "ready", model: DEFAULT_LOCAL_MODEL, lease: lateLease });
    await Promise.resolve();
    await Promise.resolve();
    const closeResult = await outcome;
    expect(settledAtDeadline).toBe(true);
    expect(closeResult).toBeInstanceOf(AggregateError);
    expect((closeResult as AggregateError).errors).toEqual([
      expect.objectContaining({ message: "runtime_activation_shutdown_timeout" }),
    ]);
    expect(closeStore).toHaveBeenCalledOnce();

    expect(lateLeaseDispose).toHaveBeenCalledOnce();
  });

  it("bounds shutdown while runtime creation is blocked and cleans late resources once", async () => {
    vi.useFakeTimers();
    const modelLease = lease();
    const modelLeaseDispose = vi.spyOn(modelLease, "dispose");
    const model = new FakeModelService(status("ready"), modelLease);
    const created = deferred<AgentRuntime>();
    const runtimeDispose = vi.fn(() => Promise.resolve());
    const lateRuntime = { dispose: runtimeDispose } as unknown as AgentRuntime;
    const createRuntime = vi.fn(() => created.promise);
    const store = openEventStore({ testOnlyMemory: true });
    const closeStore = vi.spyOn(store, "close");
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: store,
      createRuntime,
    });
    await vi.waitFor(() => expect(createRuntime).toHaveBeenCalledOnce());

    let settled = false;
    const outcome = runtime.close().then(
      () => undefined,
      (error: unknown) => error,
    );
    void outcome.then(() => {
      settled = true;
    });
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(5_000);
    const settledAtDeadline = settled;
    created.resolve(lateRuntime);
    await Promise.resolve();
    await Promise.resolve();
    const closeResult = await outcome;
    expect(settledAtDeadline).toBe(true);
    expect(closeResult).toBeInstanceOf(AggregateError);
    expect(closeStore).toHaveBeenCalledOnce();

    expect(runtimeDispose).toHaveBeenCalledOnce();
    expect(modelLeaseDispose).toHaveBeenCalledOnce();
  });

  it("continues cleanup when model unsubscription fails", async () => {
    const model = new FakeModelService(status("setup-required"), lease());
    vi.spyOn(model, "subscribe").mockReturnValue(() => {
      throw new Error("private unsubscribe failure");
    });
    const store = openEventStore({ testOnlyMemory: true });
    const closeStore = vi.spyOn(store, "close");
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: store,
    });

    await expect(runtime.close()).rejects.toBeInstanceOf(AggregateError);
    expect(model.dispose).toHaveBeenCalledOnce();
    expect(closeStore).toHaveBeenCalledOnce();
  });

  it.each([
    {
      label: "turn count",
      state: completedState(Array.from({ length: MAX_SESSION_TURNS }, () => "short")),
      next: "next turn",
    },
    {
      label: "cumulative user bytes",
      state: completedState(
        Array.from({ length: 16 }, () => "x".repeat(MAX_SESSION_USER_BYTES / 16)),
      ),
      next: "x",
    },
  ])(
    "rotates an inactive session at the $label budget and preserves old events",
    async ({ state, next }) => {
      const first = new FakeAgentRuntime(state);
      const created: FakeAgentRuntime[] = [first];
      let firstCall = true;
      const createRuntime = vi.fn(
        async (...args: Parameters<typeof createProductionRuntime>): Promise<AgentRuntime> => {
          if (firstCall) {
            firstCall = false;
            return first;
          }
          const sessionId = args[3];
          const runtime = new FakeAgentRuntime(stateFrom([{ type: "session_opened" }], sessionId));
          created.push(runtime);
          return runtime;
        },
      );
      const store = openEventStore({ testOnlyMemory: true });
      await store.append(SESSION_ID, 0n, [
        {
          eventId: "old-event",
          type: "retained_fixture",
          occurredAt: "2026-10-06T00:00:00.000Z",
          payload: { retained: true },
        },
      ]);
      const model = new FakeModelService(status("ready"), lease());
      const runtime = createModelAwareRuntime({
        sessionId: SESSION_ID,
        modelService: model,
        eventStore: store,
        createRuntime,
      });
      await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_COMPLETED);

      await expect(runtime.host.execute({ kind: "start-turn", text: next })).resolves.toEqual({
        ok: true,
      });
      expect(first.disposeCalls).toBe(1);
      expect(created.at(-1)?.starts).toEqual([next]);
      expect((await runtime.host.snapshot()).sessionId).not.toBe(SESSION_ID);
      expect(await store.currentRevision(SESSION_ID)).toBe(1n);
      await runtime.close();
    },
  );

  it("does not rotate an active over-budget session", async () => {
    const active = new FakeAgentRuntime({
      ...completedState(Array.from({ length: MAX_SESSION_TURNS }, () => "short")),
      phase: "running",
    });
    active.startError = new AgentRuntimeError("SESSION_BUSY", "active");
    const createRuntime = vi.fn(async () => active);
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: new FakeModelService(status("ready"), lease()),
      eventStore: openEventStore({ testOnlyMemory: true }),
      createRuntime,
    });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_PREPARING);

    await expect(runtime.host.execute({ kind: "start-turn", text: "next" })).resolves.toEqual({
      ok: false,
      error: { code: "TURN_ALREADY_ACTIVE" },
    });
    expect(createRuntime).toHaveBeenCalledOnce();
    expect(active.disposeCalls).toBe(0);
    await runtime.close();
  });

  it("rejects follow-ups beyond the bounded queue", async () => {
    const queued = new FakeAgentRuntime({
      ...completedState([]),
      phase: "ready",
      queuedFollowUps: Array.from({ length: MAX_QUEUED_FOLLOW_UPS }, () => "queued"),
    });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: new FakeModelService(status("ready"), lease()),
      eventStore: openEventStore({ testOnlyMemory: true }),
      createRuntime: vi.fn(async () => queued),
    });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_READY);

    await expect(
      runtime.host.execute({ kind: "queue-follow-up", text: "one more" }),
    ).resolves.toEqual({
      ok: false,
      error: { code: "RESOURCE_EXHAUSTED" },
    });
    expect(queued.followUps).toEqual([]);
    await runtime.close();
  });

  it("rejects cumulative follow-up turns before persisting them", async () => {
    const queued = new FakeAgentRuntime({
      ...completedState(Array.from({ length: MAX_SESSION_TURNS - 1 }, () => "turn")),
      phase: "ready",
      queuedFollowUps: ["reserved"],
    });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: new FakeModelService(status("ready"), lease()),
      eventStore: openEventStore({ testOnlyMemory: true }),
      createRuntime: vi.fn(async () => queued),
    });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_READY);

    await expect(
      runtime.host.execute({ kind: "queue-follow-up", text: "overflow" }),
    ).resolves.toEqual({ ok: false, error: { code: "RESOURCE_EXHAUSTED" } });
    expect(queued.followUps).toEqual([]);
    await runtime.close();
  });

  it("rejects repeated steering before persisting it", async () => {
    const completed = completedState(["active"]);
    const activeTurn = completed.turns.at(0);
    if (activeTurn === undefined) throw new Error("active turn fixture is missing");
    const active = new FakeAgentRuntime({
      ...completed,
      phase: "running",
      turns: [
        {
          ...activeTurn,
          status: "running",
          steering: Array.from({ length: 16 }, () => "steer"),
        },
      ],
    });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: new FakeModelService(status("ready"), lease()),
      eventStore: openEventStore({ testOnlyMemory: true }),
      createRuntime: vi.fn(async () => active),
    });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_PREPARING);

    await expect(
      runtime.host.execute({ kind: "steer-current-turn", text: "overflow" }),
    ).resolves.toEqual({ ok: false, error: { code: "RESOURCE_EXHAUSTED" } });
    expect(active.steering).toEqual([]);
    await runtime.close();
  });

  it("disposes the old lease after rotation failure and owns the retry lease", async () => {
    const oldLease = lease();
    const oldLeaseDispose = vi.spyOn(oldLease, "dispose");
    const retryLease = lease();
    const retryLeaseDispose = vi.spyOn(retryLease, "dispose");
    const model = new FakeModelService(status("ready"), oldLease);
    vi.mocked(model.load)
      .mockResolvedValueOnce({ kind: "ready", model: DEFAULT_LOCAL_MODEL, lease: oldLease })
      .mockResolvedValueOnce({ kind: "ready", model: DEFAULT_LOCAL_MODEL, lease: retryLease });
    const first = new FakeAgentRuntime(
      completedState(Array.from({ length: MAX_SESSION_TURNS }, () => "turn")),
    );
    const retried = new FakeAgentRuntime(stateFrom([{ type: "session_opened" }], "c".repeat(64)));
    let createCalls = 0;
    const createRuntime = vi.fn(async (): Promise<AgentRuntime> => {
      createCalls += 1;
      if (createCalls === 1) return first;
      if (createCalls === 2) throw new Error("rotation failed");
      return retried;
    });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: openEventStore({ testOnlyMemory: true }),
      createRuntime,
    });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_COMPLETED);

    await expect(runtime.host.execute({ kind: "start-turn", text: "rotate" })).resolves.toEqual({
      ok: false,
      error: { code: "RUNTIME_STOPPED" },
    });
    expect(oldLeaseDispose).toHaveBeenCalledOnce();
    await expect(
      runtime.host.execute({ kind: "start-model-download", modelId: DEFAULT_LOCAL_MODEL.id }),
    ).resolves.toEqual({ ok: true });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_READY);
    expect(oldLeaseDispose).toHaveBeenCalledOnce();
    expect(retryLeaseDispose).not.toHaveBeenCalled();

    await runtime.close();
    expect(retryLeaseDispose).toHaveBeenCalledOnce();
  });

  it("retains a failed old-lease disposal for shutdown retry", async () => {
    const oldLease = lease();
    const oldLeaseDispose = vi
      .spyOn(oldLease, "dispose")
      .mockRejectedValueOnce(new Error("old lease busy"))
      .mockResolvedValueOnce();
    const retryLease = lease();
    const model = new FakeModelService(status("ready"), oldLease);
    vi.mocked(model.load)
      .mockResolvedValueOnce({ kind: "ready", model: DEFAULT_LOCAL_MODEL, lease: oldLease })
      .mockResolvedValueOnce({ kind: "ready", model: DEFAULT_LOCAL_MODEL, lease: retryLease });
    let createCalls = 0;
    const createRuntime = vi.fn(async (): Promise<AgentRuntime> => {
      createCalls += 1;
      if (createCalls === 1) {
        return new FakeAgentRuntime(
          completedState(Array.from({ length: MAX_SESSION_TURNS }, () => "turn")),
        );
      }
      if (createCalls === 2) throw new Error("rotation failed");
      return new FakeAgentRuntime(stateFrom([{ type: "session_opened" }], "d".repeat(64)));
    });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: openEventStore({ testOnlyMemory: true }),
      createRuntime,
    });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_COMPLETED);
    await runtime.host.execute({ kind: "start-turn", text: "rotate" });
    await runtime.host.execute({ kind: "start-model-download", modelId: DEFAULT_LOCAL_MODEL.id });
    await waitFor(runtime, (phase) => phase === RuntimePhase.RUNTIME_PHASE_READY);

    await runtime.close();
    expect(oldLeaseDispose).toHaveBeenCalledTimes(2);
  });

  it("all-attempts late activation cleanup and reports its failures through close", async () => {
    const leaseDisposeFailure = new Error("lease dispose failed");
    const runtimeDisposeFailure = new Error("runtime dispose failed");
    const modelLease = lease();
    const modelLeaseDispose = vi.fn(() => Promise.reject(leaseDisposeFailure));
    modelLease.dispose = modelLeaseDispose;
    const model = new FakeModelService(status("ready"), modelLease);
    const created = deferred<AgentRuntime>();
    const runtimeDispose = vi.fn(() => Promise.reject(runtimeDisposeFailure));
    const createRuntime = vi.fn(() => created.promise);
    const store = openEventStore({ testOnlyMemory: true });
    const closeStore = vi.spyOn(store, "close");
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: model,
      eventStore: store,
      createRuntime,
    });
    await vi.waitFor(() => expect(createRuntime).toHaveBeenCalledOnce());

    const closing = runtime.close();
    created.resolve({ dispose: runtimeDispose } as unknown as AgentRuntime);
    const caught = await closing.catch((error: unknown) => error);
    expect(caught).toBeInstanceOf(AggregateError);
    const [activationFailure] = (caught as AggregateError).errors as readonly unknown[];
    expect(activationFailure).toBeInstanceOf(AggregateError);
    expect((activationFailure as AggregateError).errors).toEqual([
      runtimeDisposeFailure,
      leaseDisposeFailure,
    ]);
    expect(runtimeDispose).toHaveBeenCalledOnce();
    expect(modelLeaseDispose).toHaveBeenCalledOnce();
    expect(closeStore).toHaveBeenCalledOnce();
  });
});
