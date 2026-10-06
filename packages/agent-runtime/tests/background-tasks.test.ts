import { describe, expect, test, vi } from "vitest";

import { RuntimeBackgroundTasks } from "@lyonbrown/agent-runtime/internal/background-tasks";

describe("runtime background task supervisor", () => {
  test("removes rejected work and refuses new work after close", async () => {
    const tasks = new RuntimeBackgroundTasks();
    const failure = new Error("task failure");
    expect(
      tasks.track(() => {
        return Promise.reject(failure);
      }),
    ).toBe(true);
    await Promise.resolve();
    await Promise.resolve();

    await expect(
      tasks.close({ cancel: () => undefined, dispose: () => undefined, cancelActive: false }),
    ).resolves.toBe(false);
    expect(tasks.track(() => Promise.resolve())).toBe(false);
    await expect(
      tasks.close({ cancel: () => undefined, dispose: () => undefined, cancelActive: true }),
    ).resolves.toBe(false);
  });

  test("reports adapter failures while still attempting disposal", async () => {
    const tasks = new RuntimeBackgroundTasks();
    const cancel = vi.fn(() => {
      throw new Error("raw cancel secret");
    });
    const dispose = vi.fn(() => {
      throw new Error("raw dispose secret");
    });

    await expect(tasks.close({ cancel, dispose, cancelActive: true })).resolves.toBe(true);
    expect(cancel).toHaveBeenCalledOnce();
    expect(dispose).toHaveBeenCalledOnce();
  });

  test("fences queued work and tolerates a task settling after the drain deadline", async () => {
    const fenced = new RuntimeBackgroundTasks();
    const queuedWork = vi.fn(() => Promise.resolve());
    expect(fenced.track(queuedWork)).toBe(true);
    await fenced.close({ cancel: () => undefined, dispose: () => undefined, cancelActive: false });
    expect(queuedWork).not.toHaveBeenCalled();

    const delayed = new RuntimeBackgroundTasks();
    let release: (() => void) | undefined;
    expect(
      delayed.track(
        () =>
          new Promise<void>((resolve) => {
            release = resolve;
          }),
      ),
    ).toBe(true);
    await Promise.resolve();
    await delayed.close({ cancel: () => undefined, dispose: () => undefined, cancelActive: false });
    release?.();
    await Promise.resolve();
    await Promise.resolve();
  });
});
