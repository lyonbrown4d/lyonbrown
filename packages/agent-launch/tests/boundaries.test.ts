import type { DiscoveredDaemonCapability, DiscoveryClient, ProcessSpawner } from "../src/index.js";
import { createAgentLauncher } from "../src/index.js";
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
    sleeper.resolveDeadlineSleeps = true;
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

  test("keeps native capability and ports out of enumerable launcher state", () => {
    const discovery = new FakeDiscovery();
    const clock = new ManualClock();
    const sleeper = new AdvancingSleeper(clock);
    const launcher = launcherWith(discovery, new FakeSpawner(discovery), clock, sleeper);

    expect(Object.keys(launcher)).toEqual([]);
    expect(JSON.stringify(launcher)).not.toMatch(/endpoint|token|nativeProvider/u);
  });
});
