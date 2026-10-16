import { type ChildProcess } from "node:child_process";
import { EventEmitter } from "node:events";
import { afterEach, describe, expect, it, vi } from "vitest";

import { stopProcess, withTimeout } from "@lyonbrown/workspace/internal/real-tui-blackbox-process";

afterEach(() => vi.useRealTimers());

describe("black-box process cleanup", () => {
  it("reports an unconfirmed forced exit so callers retain the live workspace", async () => {
    vi.useFakeTimers();
    const kill = vi.fn(() => false);
    const child = Object.assign(new EventEmitter(), {
      pid: 12345,
      exitCode: null,
      signalCode: null,
      kill,
    }) as unknown as ChildProcess;
    const assertion = expect(stopProcess(child, 10)).rejects.toThrow("forced shutdown timed out");
    await vi.advanceTimersByTimeAsync(20);
    await assertion;
    expect(kill.mock.calls).toEqual([["SIGTERM"], ["SIGKILL"]]);
  });

  it("does not wait for exit after spawn failed without assigning a pid", async () => {
    const kill = vi.fn(() => false);
    const child = Object.assign(new EventEmitter(), {
      pid: undefined,
      kill,
    }) as unknown as ChildProcess;
    await stopProcess(child, 10);
    expect(kill).not.toHaveBeenCalled();
  });

  it("clears a deadline when the operation finishes", async () => {
    vi.useFakeTimers();
    await expect(withTimeout(Promise.resolve("done"), 100, "expired")).resolves.toBe("done");
    expect(vi.getTimerCount()).toBe(0);
  });
});
