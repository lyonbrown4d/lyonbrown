import { deriveScope } from "@lyonbrown/local-control-plane";
import { describe, expect, test } from "vitest";
import { createAgentLauncher } from "@lyonbrown/agent-launch/internal/launcher";
import type { DaemonExit, SpawnAttempt } from "@lyonbrown/agent-launch/internal/types";
import {
  CONFIG,
  WORKSPACE,
  AdvancingSleeper,
  Candidate,
  FakeClientFactory,
  FakeDiscovery,
  FakeSpawner,
  ManualClock,
  nativeProvider,
} from "@lyonbrown/agent-launch/test/support/fixtures";

async function setup() {
  const provider = nativeProvider();
  const scope = await deriveScope(WORKSPACE, CONFIG, provider);
  const discovery = new FakeDiscovery();
  const spawner = new FakeSpawner(discovery);
  const clock = new ManualClock();
  const sleeper = new AdvancingSleeper(clock);
  const launcher = createAgentLauncher({
    nativeProvider: provider,
    workspace: WORKSPACE,
    effectiveConfigFingerprint: CONFIG,
    ports: {
      discovery,
      spawner,
      clients: new FakeClientFactory(),
      clock,
      sleeper,
    },
    deadlines: { startupMs: 15, pollIntervalMs: 5, cleanupMs: 1 },
    client: { name: "agent-test", version: "0.0.0" },
  });
  return { scope, discovery, spawner, sleeper, launcher };
}

function deferred<T>(): { readonly promise: Promise<T>; readonly resolve: (value: T) => void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

async function drainMicrotasks(): Promise<void> {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

describe("spawn quarantine", () => {
  test.each(["reject", "never"] as const)(
    "keeps the startup lease when abandon %s settles unsafely",
    async (mode) => {
      const fixture = await setup();
      if (mode === "reject") fixture.spawner.abandonFailure = new Error("private abandon failure");
      else fixture.spawner.abandonNeverSettles = true;

      await expect(fixture.launcher.connect()).rejects.toMatchObject({
        code: "READINESS_TIMEOUT",
      });
      await expect(fixture.launcher.connect()).rejects.toMatchObject({
        code: "STARTUP_TIMEOUT",
      });

      expect(fixture.spawner.calls).toHaveLength(1);
      expect(fixture.discovery.leaseHeld).toBe(true);
    },
  );

  test("holds election while a cancelled attempt commits late", async () => {
    const fixture = await setup();
    fixture.spawner.abandonNeverSettles = true;

    await expect(fixture.launcher.connect()).rejects.toMatchObject({
      code: "READINESS_TIMEOUT",
    });
    let published = false;
    fixture.sleeper.onSleep = () => {
      if (published) return;
      published = true;
      fixture.discovery.candidate = new Candidate("epoch-late", fixture.scope.scopeId);
    };

    const connection = await fixture.launcher.connect();

    expect(connection.serverInstanceId).toBe("epoch-late");
    expect(fixture.spawner.calls).toHaveLength(1);
    expect(fixture.discovery.leaseHeld).toBe(true);
  });

  test("releases quarantine only after the abandoned process explicitly exits", async () => {
    const fixture = await setup();
    const exit = deferred<DaemonExit>();
    fixture.spawner.exit = exit.promise;
    fixture.spawner.abandonFailure = new Error("private abandon failure");

    await expect(fixture.launcher.connect()).rejects.toMatchObject({
      code: "READINESS_TIMEOUT",
    });
    expect(fixture.discovery.leaseHeld).toBe(true);

    exit.resolve({ exitCode: 1 });
    await drainMicrotasks();

    expect(fixture.discovery.leaseHeld).toBe(false);
  });

  test("does not release quarantine for an invalid empty attempt handle", async () => {
    const fixture = await setup();
    fixture.spawner.begin = () =>
      ({
        exited: undefined,
        retired: undefined,
        abandon: () => Promise.reject(new Error("private invalid handle")),
      }) as unknown as SpawnAttempt;

    await expect(fixture.launcher.connect()).rejects.toMatchObject({ code: "DAEMON_EXITED" });
    await drainMicrotasks();

    expect(fixture.discovery.leaseHeld).toBe(true);
  });
});
