import { describe, expect, it, vi } from "vitest";
import { ReplayCache, ReplayConflictError } from "../node-src/replay-cache.ts";

describe("ReplayCache", () => {
  it("coalesces concurrent commands and replays the completed result", async () => {
    const cache = new ReplayCache<number>(2);
    let release = (): void => undefined;
    const pending = new Promise<number>((resolve) => {
      release = () => {
        resolve(7);
      };
    });
    const execute = vi.fn(() => pending);

    const first = cache.execute("client\0command", "same", execute);
    const second = cache.execute("client\0command", "same", execute);
    expect(execute).toHaveBeenCalledTimes(1);
    release();
    await expect(Promise.all([first, second])).resolves.toEqual([7, 7]);
    await expect(cache.execute("client\0command", "same", execute)).resolves.toBe(7);
    expect(execute).toHaveBeenCalledTimes(1);
  });

  it("rejects command-id reuse with a different fingerprint", async () => {
    const cache = new ReplayCache<number>(2);
    const first = cache.execute("client\0command", "one", async () => 1);
    await expect(cache.execute("client\0command", "two", async () => 2)).rejects.toBeInstanceOf(
      ReplayConflictError,
    );
    await expect(first).resolves.toBe(1);
  });

  it("evicts only completed entries and remains bounded", async () => {
    const cache = new ReplayCache<number>(1);
    await cache.execute("a", "one", async () => 1);
    await cache.execute("b", "two", async () => 2);
    expect(cache.size).toBe(1);
    await cache.execute("a", "one", async () => 3);
    expect(cache.size).toBe(1);
  });
});
