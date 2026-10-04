import * as publicApi from "../src/index.js";
import { createAgentLauncher } from "../src/launcher.js";
import type {
  DiscoveredDaemonCapability,
  DiscoveryClient,
  ProcessSpawner,
  SpawnedDaemon,
  StartupLease,
} from "../src/types.js";
import { describe, expect, test } from "vitest";
import {
  CONFIG,
  WORKSPACE,
  AdvancingSleeper,
  FakeClientFactory,
  FakeDiscovery,
  FakeSpawner,
  ManualClock,
  nativeProvider,
} from "./support/fixtures.js";

function pending<T>(): Promise<T> {
  return new Promise<T>(() => undefined);
}

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

async function drainMicrotasks(): Promise<void> {
  for (let index = 0; index < 5; index += 1) await Promise.resolve();
}

function launcherWith(
  discovery: DiscoveryClient,
  spawner: ProcessSpawner,
  clock: ManualClock,
  sleeper: AdvancingSleeper,
) {
  return createAgentLauncher({
    nativeProvider: nativeProvider(),
    workspace: WORKSPACE,
    effectiveConfigFingerprint: CONFIG,
    ports: { discovery, spawner, clients: new FakeClientFactory(), clock, sleeper },
    deadlines: { startupMs: 20, pollIntervalMs: 5 },
    client: { name: "agent-test", version: "0.0.0" },
  });
}

describe("stage cancellation and deadlines", () => {
  test("cancels a pending discovery read", async () => {
    const controller = new AbortController();
    const discovery: DiscoveryClient = {
      read: () => {
        queueMicrotask(() => {
          controller.abort();
        });
        return pending<DiscoveredDaemonCapability | null>();
      },
      tryAcquireStartupLease: () => Promise.resolve(null),
    };
    const clock = new ManualClock();
    const sleeper = new AdvancingSleeper(clock);
    const fallback = new FakeDiscovery();

    await expect(
      launcherWith(discovery, new FakeSpawner(fallback), clock, sleeper).connect({
        signal: controller.signal,
      }),
    ).rejects.toMatchObject({ code: "CANCELLED" });
  });

  test("times out a pending discovery read", async () => {
    const discovery: DiscoveryClient = {
      read: () => pending<DiscoveredDaemonCapability | null>(),
      tryAcquireStartupLease: () => Promise.resolve(null),
    };
    const clock = new ManualClock();
    const sleeper = new AdvancingSleeper(clock);
    sleeper.resolveDeadlineSleepCalls.add(2);
    const fallback = new FakeDiscovery();

    await expect(
      launcherWith(discovery, new FakeSpawner(fallback), clock, sleeper).connect(),
    ).rejects.toMatchObject({ code: "STARTUP_TIMEOUT" });
  });

  test("cancels a pending spawn after the startup lease is acquired", async () => {
    const controller = new AbortController();
    const discovery = new FakeDiscovery();
    const spawner: ProcessSpawner = {
      spawn: () => {
        queueMicrotask(() => {
          controller.abort();
        });
        return pending();
      },
    };
    const clock = new ManualClock();
    const sleeper = new AdvancingSleeper(clock);

    await expect(
      launcherWith(discovery, spawner, clock, sleeper).connect({ signal: controller.signal }),
    ).rejects.toMatchObject({ code: "CANCELLED" });
    expect(discovery.leaseHeld).toBe(false);
  });

  test("times out a pending spawn and releases the startup lease", async () => {
    const discovery = new FakeDiscovery();
    const clock = new ManualClock();
    const sleeper = new AdvancingSleeper(clock);
    const spawner: ProcessSpawner = {
      spawn: () => {
        sleeper.resolveDeadlineSleeps = true;
        return pending();
      },
    };

    await expect(launcherWith(discovery, spawner, clock, sleeper).connect()).rejects.toMatchObject({
      code: "READINESS_TIMEOUT",
    });
    expect(discovery.leaseHeld).toBe(false);
  });

  test("releases a startup lease that resolves after its acquisition timed out", async () => {
    const lease = deferred<StartupLease | null>();
    let acquisitionSignal: AbortSignal | undefined;
    let released = false;
    const discovery: DiscoveryClient = {
      read: () => Promise.resolve(null),
      tryAcquireStartupLease: (_context, signal?: AbortSignal) => {
        acquisitionSignal = signal;
        return lease.promise;
      },
    };
    const clock = new ManualClock();
    const sleeper = new AdvancingSleeper(clock);
    sleeper.resolveDeadlineSleepCalls.add(3);
    const fallback = new FakeDiscovery();

    await expect(
      launcherWith(discovery, new FakeSpawner(fallback), clock, sleeper).connect(),
    ).rejects.toMatchObject({ code: "STARTUP_TIMEOUT" });
    lease.resolve({
      release: () => {
        released = true;
        return Promise.resolve();
      },
    });
    await drainMicrotasks();

    expect(acquisitionSignal?.aborted).toBe(true);
    expect(released).toBe(true);
  });

  test("releases a startup lease that resolves after acquisition is cancelled", async () => {
    const controller = new AbortController();
    const lease = deferred<StartupLease | null>();
    let acquisitionSignal: AbortSignal | undefined;
    let released = false;
    const discovery: DiscoveryClient = {
      read: () => Promise.resolve(null),
      tryAcquireStartupLease: (_context, signal?: AbortSignal) => {
        acquisitionSignal = signal;
        queueMicrotask(() => {
          controller.abort();
        });
        return lease.promise;
      },
    };
    const clock = new ManualClock();
    const sleeper = new AdvancingSleeper(clock);
    const fallback = new FakeDiscovery();

    await expect(
      launcherWith(discovery, new FakeSpawner(fallback), clock, sleeper).connect({
        signal: controller.signal,
      }),
    ).rejects.toMatchObject({ code: "CANCELLED" });
    lease.resolve({
      release: () => {
        released = true;
        return Promise.resolve();
      },
    });
    await drainMicrotasks();

    expect(acquisitionSignal?.aborted).toBe(true);
    expect(released).toBe(true);
  });

  test.each(["timeout", "abort"] as const)(
    "abandons a spawn that resolves after %s",
    async (mode) => {
      const controller = new AbortController();
      const discovery = new FakeDiscovery();
      const spawned = deferred<SpawnedDaemon>();
      let spawnSignal: AbortSignal | undefined;
      let abandoned = false;
      const spawner: ProcessSpawner = {
        spawn: (_request, signal?: AbortSignal) => {
          spawnSignal = signal;
          if (mode === "abort") {
            queueMicrotask(() => {
              controller.abort();
            });
          }
          return spawned.promise;
        },
      };
      const clock = new ManualClock();
      const sleeper = new AdvancingSleeper(clock);
      if (mode === "timeout") sleeper.resolveDeadlineSleepCalls.add(5);
      const connection = launcherWith(discovery, spawner, clock, sleeper).connect({
        signal: controller.signal,
      });

      await expect(connection).rejects.toMatchObject({
        code: mode === "timeout" ? "READINESS_TIMEOUT" : "CANCELLED",
      });
      spawned.resolve({
        exited: pending(),
        abandon: () => {
          abandoned = true;
          return Promise.resolve();
        },
      } as SpawnedDaemon & { abandon(): Promise<void> });
      await drainMicrotasks();

      expect(spawnSignal?.aborted).toBe(true);
      expect(abandoned).toBe(true);
      expect(discovery.leaseHeld).toBe(false);
    },
  );

  test("keeps native capability and ports out of enumerable launcher state", () => {
    const discovery = new FakeDiscovery();
    const clock = new ManualClock();
    const sleeper = new AdvancingSleeper(clock);
    const launcher = launcherWith(discovery, new FakeSpawner(discovery), clock, sleeper);

    expect(Object.keys(launcher)).toEqual([]);
    expect(JSON.stringify(launcher)).not.toMatch(/endpoint|token|nativeProvider/u);
  });
});

describe("public entry point", () => {
  test("exports only the client-facing runtime value", () => {
    expect(Object.keys(publicApi)).toEqual(["AgentLaunchError"]);
  });
});
