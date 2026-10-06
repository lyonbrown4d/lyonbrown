import { describe, expect, test } from "vitest";
import { SpawnQuarantine } from "@lyonbrown/agent-launch/internal/spawn-quarantine";
import { StageRunner } from "@lyonbrown/agent-launch/internal/stage-runner";
import type { SpawnAttempt, StartupLease } from "@lyonbrown/agent-launch/internal/types";
import { AdvancingSleeper, ManualClock } from "@lyonbrown/agent-launch/test/support/fixtures";

function deferred<T>(): {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
} {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function quarantine(): SpawnQuarantine {
  const clock = new ManualClock();
  return new SpawnQuarantine(new StageRunner(clock, new AdvancingSleeper(clock), 5, 1));
}

async function drainMicrotasks(): Promise<void> {
  for (let index = 0; index < 12; index += 1) await Promise.resolve();
}

async function forceGarbageCollection(): Promise<void> {
  const gc = (globalThis as typeof globalThis & { gc?: () => void }).gc;
  expect(gc, "tests must run Node with --expose-gc").toBeTypeOf("function");
  await new Promise<void>((resolve) => setImmediate(resolve));
  for (let index = 0; index < 8; index += 1) {
    gc?.();
    await new Promise<void>((resolve) => setImmediate(resolve));
  }
}

function startPending(quarantineOwner: SpawnQuarantine): WeakRef<object> {
  const terminal = new Promise<never>(() => undefined);
  const attempt: SpawnAttempt = {
    retired: terminal,
    exited: terminal,
    abandon: () => terminal,
  };
  const weakAttempt = new WeakRef<object>(attempt);
  quarantineOwner.start({ release: () => terminal }, attempt);
  return weakAttempt;
}

function startTracked(
  quarantineOwner: SpawnQuarantine,
  lease: StartupLease,
  attempt: SpawnAttempt,
): WeakRef<object> {
  const weakAttempt = new WeakRef<object>(attempt);
  quarantineOwner.start(lease, attempt);
  return weakAttempt;
}

describe("spawn quarantine ownership", () => {
  test("strongly owns a pending attempt across forced garbage collection", async () => {
    const owner = quarantine();
    const weakAttempt = startPending(owner);

    await forceGarbageCollection();

    expect(owner.retainedCountForTesting()).toBe(1);
    expect(weakAttempt.deref()).toBeDefined();
  });

  test("retains rejected and hostile terminal signals", async () => {
    const owner = quarantine();
    const rejected = Promise.reject(new Error("private terminal failure"));
    owner.start(
      { release: () => Promise.resolve() },
      { retired: rejected, exited: rejected, abandon: () => Promise.resolve() },
    );
    const hostile = {} as SpawnAttempt;
    Object.defineProperties(hostile, {
      retired: { get: () => Promise.reject(new Error("hostile retired getter")) },
      exited: {
        get: () => {
          throw new Error("hostile exited getter");
        },
      },
      abandon: { value: () => Promise.resolve() },
    });
    owner.start({ release: () => Promise.resolve() }, hostile);

    await drainMicrotasks();

    expect(owner.retainedCountForTesting()).toBe(2);
  });

  test.each(["reject", "never"] as const)(
    "retains a safely terminated attempt when lease release will %s settle safely",
    async (mode) => {
      const owner = quarantine();
      const retired = deferred<undefined>();
      const never = new Promise<never>(() => undefined);
      const lease: StartupLease = {
        release: () =>
          mode === "reject" ? Promise.reject(new Error("private release failure")) : never,
      };
      const weakAttempt = startTracked(owner, lease, {
        retired: retired.promise,
        exited: never,
        abandon: () => Promise.resolve(),
      });

      retired.resolve(undefined);
      await drainMicrotasks();
      await forceGarbageCollection();

      expect(owner.retainedCountForTesting()).toBe(1);
      expect(weakAttempt.deref()).toBeDefined();
    },
  );

  test("removes ownership only after termination and confirmed lease release", async () => {
    const owner = quarantine();
    const retired = deferred<undefined>();
    owner.start(
      { release: () => Promise.resolve() },
      {
        retired: retired.promise,
        exited: new Promise<never>(() => undefined),
        abandon: () => Promise.resolve(),
      },
    );
    expect(owner.retainedCountForTesting()).toBe(1);

    retired.resolve(undefined);
    await drainMicrotasks();

    expect(owner.retainedCountForTesting()).toBe(0);
  });
});
