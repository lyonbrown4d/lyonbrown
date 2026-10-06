import { openEventStore } from "@lyonbrown/event-store";
import type { LocalModelLease, LocalModelMessage } from "@lyonbrown/local-model";
import { describe, expect, it, vi } from "vitest";

import { createProductionRuntime } from "@lyonbrown/agentd/internal/production-kernel";

const OLD_SESSION = "old-session";

function completedLease(requests: LocalModelMessage[][] = []): LocalModelLease {
  return {
    async *stream(request) {
      requests.push([...request.messages]);
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

  it("hydrates and resumes a completed session after restart", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const requests: LocalModelMessage[][] = [];
    const lease = completedLease(requests);
    const first = await createProductionRuntime(lease, "model", store, OLD_SESSION);
    await first.startTurn("finish normally");
    await waitForPhase(first, "completed");
    await first.dispose();

    const restarted = await createProductionRuntime(lease, "model", store, OLD_SESSION);
    expect(restarted.snapshot().sessionId).toBe(OLD_SESSION);
    expect(await store.currentRevision(OLD_SESSION)).toBeGreaterThan(1n);
    await restarted.startTurn("new context");
    await waitForPhase(restarted, "completed");
    expect(requests[1]).toEqual([
      expect.objectContaining({ role: "system" }),
      { role: "user", content: "finish normally" },
      { role: "assistant", content: "done" },
      { role: "user", content: "new context" },
    ]);
    await restarted.dispose();
    store.close();
  });

  it("rotates completed history that exceeds the Kernel JSON budget after escaping", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const lease = completedLease();
    const first = await createProductionRuntime(lease, "model", store, OLD_SESSION);
    for (let turn = 0; turn < 4; turn += 1) {
      await first.startTurn("\\".repeat(64_000));
      await waitForPhase(first, "completed");
    }
    await first.dispose();

    const restarted = await createProductionRuntime(lease, "model", store, OLD_SESSION);
    expect(restarted.snapshot().sessionId).not.toBe(OLD_SESSION);
    await restarted.dispose();
    store.close();
  });
});
