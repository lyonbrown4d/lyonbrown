import { EventEmitter } from "node:events";
import { describe, expect, it, vi } from "vitest";

import { waitForShutdownSignal } from "@lyonbrown/agentd/internal/process-lifecycle";

describe("agentd process lifecycle", () => {
  it("closes once on the first termination signal and removes both listeners", async () => {
    const signals = new EventEmitter();
    const close = vi.fn(async () => undefined);
    const waiting = waitForShutdownSignal({ close }, signals);

    signals.emit("SIGTERM");
    signals.emit("SIGINT");

    await expect(waiting).resolves.toBeUndefined();
    expect(close).toHaveBeenCalledOnce();
    expect(signals.listenerCount("SIGINT")).toBe(0);
    expect(signals.listenerCount("SIGTERM")).toBe(0);
  });

  it("propagates shutdown failure after removing listeners", async () => {
    const signals = new EventEmitter();
    const failure = new Error("close failed");
    const waiting = waitForShutdownSignal({ close: async () => Promise.reject(failure) }, signals);

    signals.emit("SIGINT");

    await expect(waiting).rejects.toBe(failure);
    expect(signals.listenerCount("SIGINT")).toBe(0);
    expect(signals.listenerCount("SIGTERM")).toBe(0);
  });
});
