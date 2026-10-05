import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import type { KernelSession } from "@lyonbrown/kernel-pi";
import {
  createAgentRuntime,
  createRuntimeEventStorePort,
  type RuntimeEventStorePort,
} from "@lyonbrown/agent-runtime";
import { describe, expect, test, vi } from "vitest";

import { createRuntimeHostAdapter } from "../node-src/runtime-adapter.ts";

function deferred(): { readonly promise: Promise<void>; readonly resolve: () => void } {
  let resolve = (): void => undefined;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function eventType(value: unknown): string | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const type = (value as { readonly type?: unknown }).type;
  return typeof type === "string" ? type : undefined;
}

function blockEvent(
  base: RuntimeEventStorePort,
  type: string,
): {
  readonly port: RuntimeEventStorePort;
  readonly reached: Promise<void>;
  readonly release: () => void;
} {
  const reached = deferred();
  const release = deferred();
  let blocked = false;
  return {
    port: {
      ...base,
      append: async (sessionId, revision, events) => {
        if (!blocked && eventType(events[0]?.payload) === type) {
          blocked = true;
          reached.resolve();
          await release.promise;
        }
        return base.append(sessionId, revision, events);
      },
    },
    reached: reached.promise,
    release: release.resolve,
  };
}

describe("runtime host control linearization", () => {
  test.each(["steer-current-turn", "cancel-turn"] as const)(
    "accepts %s while the initial user message append is pending",
    async (kind) => {
      const store = openEventStore({ testOnlyMemory: true });
      const blocked = blockEvent(createRuntimeEventStorePort(store), "user_message_submitted");
      const kernelRelease = deferred();
      const steer = vi.fn();
      const cancel = vi.fn();
      const starts: string[] = [];
      const sessionId = asSessionId(`adapter-pending-${kind}`);
      const runtime = await createAgentRuntime({
        sessionId,
        eventStore: blocked.port,
        createKernelSession: ({ eventSink }): KernelSession => ({
          async startTurn(input) {
            starts.push(input);
            await eventSink.append({ type: "run_started", sessionId });
            await kernelRelease.promise;
            await eventSink.append({ type: "run_completed", sessionId });
          },
          steer,
          followUp: () => undefined,
          cancel,
          subscribe: () => () => undefined,
          dispose: () => undefined,
        }),
      });
      const host = createRuntimeHostAdapter(runtime);
      const start = host.execute({ kind: "start-turn", text: "question" });
      await blocked.reached;

      const control =
        kind === "steer-current-turn"
          ? host.execute({ kind, text: "focus" })
          : host.execute({ kind });
      blocked.release();

      await expect(control).resolves.toEqual({ ok: true });
      await expect(start).resolves.toEqual({ ok: true });
      if (kind === "cancel-turn") {
        await expect.poll(() => runtime.snapshot().phase).toBe("cancelled");
        expect(starts).toEqual([]);
        expect(cancel).toHaveBeenCalledOnce();
      } else {
        await expect.poll(() => steer).toHaveBeenCalledWith("focus");
        kernelRelease.resolve();
        await expect.poll(() => runtime.snapshot().phase).toBe("completed");
      }

      await runtime.dispose();
      store.close();
    },
  );

  test("lets controls join a pending durable follow-up claim", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const blocked = blockEvent(createRuntimeEventStorePort(store), "follow_up_started");
    const steer = vi.fn();
    const cancel = vi.fn();
    let starts = 0;
    const sessionId = asSessionId("adapter-pending-follow-up");
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore: blocked.port,
      createKernelSession: ({ eventSink }): KernelSession => ({
        async startTurn() {
          starts += 1;
          await eventSink.append({ type: "run_started", sessionId });
          await eventSink.append({ type: "run_completed", sessionId });
        },
        steer,
        followUp: () => undefined,
        cancel,
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });
    const host = createRuntimeHostAdapter(runtime);
    await host.execute({ kind: "start-turn", text: "first" });
    await expect.poll(() => runtime.snapshot().phase).toBe("completed");
    await host.execute({ kind: "queue-follow-up", text: "second" });
    await blocked.reached;

    const steering = host.execute({ kind: "steer-current-turn", text: "focus" });
    const cancellation = host.execute({ kind: "cancel-turn" });
    blocked.release();

    await expect(steering).resolves.toEqual({ ok: true });
    await expect(cancellation).resolves.toEqual({ ok: true });
    await expect.poll(() => runtime.snapshot().phase).toBe("cancelled");
    expect(starts).toBe(1);
    expect(steer).toHaveBeenCalledWith("focus");
    expect(cancel).toHaveBeenCalledOnce();
    await runtime.dispose();
    store.close();
  });

  test("distinguishes idle, durable terminal, and recovered unresolved controls", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const kernelRelease = deferred();
    const steer = vi.fn();
    const cancel = vi.fn();
    const eventStore = createRuntimeEventStorePort(store);
    const sessionId = asSessionId("adapter-control-boundaries");
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore,
      createKernelSession: ({ eventSink }): KernelSession => ({
        async startTurn() {
          await eventSink.append({ type: "run_started", sessionId });
          await eventSink.append({ type: "run_completed", sessionId });
          await kernelRelease.promise;
        },
        steer,
        followUp: () => undefined,
        cancel,
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });
    const host = createRuntimeHostAdapter(runtime);

    await expect(host.execute({ kind: "steer-current-turn", text: "idle" })).resolves.toEqual({
      ok: false,
      error: { code: "NO_ACTIVE_TURN" },
    });
    await expect(host.execute({ kind: "cancel-turn" })).resolves.toEqual({
      ok: false,
      error: { code: "NO_ACTIVE_TURN" },
    });
    await host.execute({ kind: "start-turn", text: "first" });
    await expect.poll(() => runtime.snapshot().phase).toBe("completed");
    await expect(host.execute({ kind: "steer-current-turn", text: "late" })).resolves.toEqual({
      ok: false,
      error: { code: "RUNTIME_STOPPED" },
    });
    await expect(host.execute({ kind: "cancel-turn" })).resolves.toEqual({
      ok: false,
      error: { code: "RUNTIME_STOPPED" },
    });
    expect(steer).not.toHaveBeenCalled();
    expect(cancel).not.toHaveBeenCalled();

    kernelRelease.resolve();
    await runtime.dispose();
    store.close();
  });
});
