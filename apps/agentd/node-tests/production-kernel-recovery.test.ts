import { openEventStore } from "@lyonbrown/event-store";
import type { LocalModelLease } from "@lyonbrown/local-model";
import { describe, expect, it, vi } from "vitest";

import { createProductionRuntime } from "../node-src/production-kernel.ts";

const OLD_SESSION = "old-session";

function completedLease(): LocalModelLease {
  return {
    async *stream() {
      yield { type: "text_delta", text: "done" } as const;
      yield { type: "usage", inputTokens: 1, outputTokens: 1 } as const;
      yield { type: "done", reason: "stop" } as const;
    },
    dispose: vi.fn(() => Promise.resolve()),
  };
}

function blockedLease(): LocalModelLease {
  return {
    async *stream(request) {
      await new Promise<void>((resolve) => {
        if (request.signal.aborted) resolve();
        else request.signal.addEventListener("abort", () => resolve(), { once: true });
      });
      yield { type: "done", reason: "cancelled" } as const;
    },
    dispose: vi.fn(() => Promise.resolve()),
  };
}

async function waitForPhase(
  runtime: Awaited<ReturnType<typeof createProductionRuntime>>,
  phase: "running" | "completed",
): Promise<void> {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (runtime.snapshot().phase === phase) return;
    await new Promise<void>((resolve) => setTimeout(resolve, 5));
  }
  throw new Error("runtime phase did not converge");
}

describe("production kernel crash recovery", () => {
  it("preserves an unsafe mid-turn session and starts a fresh safe session", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const lease = blockedLease();
    const crashed = await createProductionRuntime(lease, "model", store, OLD_SESSION);
    await crashed.startTurn("work in progress");
    await waitForPhase(crashed, "running");

    const restarted = await createProductionRuntime(lease, "model", store, OLD_SESSION);
    try {
      expect(restarted.snapshot().sessionId).not.toBe(OLD_SESSION);
      expect(await store.currentRevision(OLD_SESSION)).toBeGreaterThan(1n);
      await expect(restarted.startTurn("safe restart")).resolves.toBeUndefined();
    } finally {
      await restarted.dispose();
      await crashed.dispose();
      store.close();
    }
  });

  it("rotates a completed session until Pi history hydration is supported", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const lease = completedLease();
    const first = await createProductionRuntime(lease, "model", store, OLD_SESSION);
    await first.startTurn("finish normally");
    await waitForPhase(first, "completed");
    await first.dispose();

    const restarted = await createProductionRuntime(lease, "model", store, OLD_SESSION);
    expect(restarted.snapshot().sessionId).not.toBe(OLD_SESSION);
    expect(await store.currentRevision(OLD_SESSION)).toBeGreaterThan(1n);
    await restarted.startTurn("new context");
    await waitForPhase(restarted, "completed");
    await restarted.dispose();
    store.close();
  });
});
