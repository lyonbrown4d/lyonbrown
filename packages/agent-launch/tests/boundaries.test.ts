import * as publicApi from "../src/index.js";
import { createAgentLauncher } from "../src/launcher.js";
import type {
  DiscoveredDaemonCapability,
  DiscoveryClient,
  ProcessSpawner,
  SpawnAttempt,
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

  test("cancels an active spawn attempt after the startup lease is acquired", async () => {
    const controller = new AbortController();
    const discovery = new FakeDiscovery();
    let retire!: () => void;
    const retired = new Promise<void>((resolve) => {
      retire = resolve;
    });
    const spawner: ProcessSpawner = {
      begin: (): SpawnAttempt => {
        queueMicrotask(() => {
          controller.abort();
        });
        return {
          exited: pending(),
          retired,
          abandon: () => {
            retire();
            return Promise.resolve();
          },
        };
      },
    };
    const clock = new ManualClock();
    const sleeper = new AdvancingSleeper(clock);

    await expect(
      launcherWith(discovery, spawner, clock, sleeper).connect({ signal: controller.signal }),
    ).rejects.toMatchObject({ code: "CANCELLED" });
    await drainMicrotasks();
    expect(discovery.leaseHeld).toBe(false);
  });

  test("times out attempt readiness and releases after confirmed retirement", async () => {
    const discovery = new FakeDiscovery();
    const clock = new ManualClock();
    const sleeper = new AdvancingSleeper(clock);
    const spawner: ProcessSpawner = {
      begin: (): SpawnAttempt => {
        sleeper.resolveDeadlineSleeps = true;
        return {
          exited: pending(),
          retired: Promise.resolve(),
          abandon: () => Promise.resolve(),
        };
      },
    };

    await expect(launcherWith(discovery, spawner, clock, sleeper).connect()).rejects.toMatchObject({
      code: "READINESS_TIMEOUT",
    });
    await drainMicrotasks();
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
