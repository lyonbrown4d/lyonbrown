import { deriveScope } from "@lyonbrown/local-control-plane";
import { describe, expect, test } from "vitest";
import { AgentLaunchError } from "@lyonbrown/agent-launch/internal/errors";
import { createAgentLauncher } from "@lyonbrown/agent-launch/internal/launcher";
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

async function setup(startupMs = 100) {
  const provider = nativeProvider();
  const scope = await deriveScope(WORKSPACE, CONFIG, provider);
  const discovery = new FakeDiscovery();
  const clients = new FakeClientFactory();
  const spawner = new FakeSpawner(discovery);
  const clock = new ManualClock();
  const sleeper = new AdvancingSleeper(clock);
  const launcher = createAgentLauncher({
    nativeProvider: provider,
    workspace: WORKSPACE,
    effectiveConfigFingerprint: CONFIG,
    ports: { discovery, spawner, clients, clock, sleeper },
    deadlines: { startupMs, pollIntervalMs: 5 },
    client: { name: "agent-test", version: "0.0.0" },
  });
  return { scope, discovery, clients, spawner, clock, sleeper, launcher };
}

describe("daemon startup coordination", () => {
  test("startup lease winner spawns once and waits for authenticated readiness", async () => {
    const fixture = await setup();
    fixture.spawner.candidateToPublish = new Candidate("epoch-new", fixture.scope.scopeId);

    const connection = await fixture.launcher.connect();

    expect(connection.source).toBe("spawned");
    expect(fixture.spawner.calls).toHaveLength(1);
    expect(fixture.spawner.calls[0]).toEqual({
      canonicalWorkspace: fixture.scope.canonicalWorkspace,
      effectiveConfigFingerprint: CONFIG,
      scopeId: fixture.scope.scopeId,
    });
    expect(Object.keys(fixture.spawner.calls[0] ?? {})).not.toContain("token");
    expect(fixture.discovery.leaseHeld).toBe(false);
  });

  test("two concurrent first clients cause exactly one spawn", async () => {
    const fixture = await setup();
    fixture.spawner.candidateToPublish = new Candidate("epoch-new", fixture.scope.scopeId);

    const [first, second] = await Promise.all([
      fixture.launcher.connect(),
      fixture.launcher.connect(),
    ]);

    expect(first.serverInstanceId).toBe("epoch-new");
    expect(second.serverInstanceId).toBe("epoch-new");
    expect(fixture.spawner.calls).toHaveLength(1);
    expect(fixture.clients.clients).toHaveLength(2);
    expect(fixture.clients.clients.map((client) => client.handshakeCalls)).toEqual([1, 1]);
  });

  test("does not start when automatic startup is disabled", async () => {
    const fixture = await setup();

    await expect(fixture.launcher.connect({ autoStart: false })).rejects.toMatchObject({
      code: "AUTO_START_DISABLED",
    });
    expect(fixture.spawner.calls).toHaveLength(0);
  });

  test("reports a daemon exit with a typed safe error", async () => {
    const fixture = await setup();
    fixture.spawner.exit = Promise.resolve({ exitCode: 78 });

    const error = await fixture.launcher.connect().catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(AgentLaunchError);
    expect(error).toMatchObject({ code: "DAEMON_EXITED", details: { exitCode: 78 } });
    expect(error).not.toHaveProperty("cause");
    expect(fixture.discovery.leaseHeld).toBe(false);
  });

  test("maps a rejected daemon exit observation to a typed safe error", async () => {
    const fixture = await setup();
    fixture.spawner.exit = new Promise((_, reject) => {
      queueMicrotask(() => {
        reject(new Error("raw exit observer secret"));
      });
    });

    const error = await fixture.launcher.connect().catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(AgentLaunchError);
    expect(error).toMatchObject({ code: "DAEMON_EXITED" });
    expect(String(error)).not.toContain("secret");
    expect(error).not.toHaveProperty("cause");
  });

  test("guards hostile daemon exit fields", async () => {
    const fixture = await setup();
    const hostileExit = new Proxy(Object.create(null) as { readonly exitCode: number | null }, {
      get() {
        throw new Error("raw exit getter secret");
      },
    });
    fixture.spawner.exit = Promise.resolve(hostileExit);

    const error = await fixture.launcher.connect().catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(AgentLaunchError);
    expect(error).toMatchObject({ code: "DAEMON_EXITED" });
    expect(String(error)).not.toContain("secret");
    expect(error).not.toHaveProperty("cause");
  });

  test("times out, releases the startup lease, and does not kill a stale process", async () => {
    const fixture = await setup(15);

    await expect(fixture.launcher.connect()).rejects.toMatchObject({ code: "READINESS_TIMEOUT" });
    expect(fixture.spawner.calls).toHaveLength(1);
    expect(fixture.discovery.leaseHeld).toBe(false);
  });

  test("honors cancellation and releases the startup lease", async () => {
    const fixture = await setup();
    const controller = new AbortController();
    fixture.sleeper.onSleep = () => {
      controller.abort(new Error("raw abort reason"));
    };

    const error = await fixture.launcher
      .connect({ signal: controller.signal })
      .catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(AgentLaunchError);
    expect(error).toMatchObject({ code: "CANCELLED" });
    expect(String(error)).not.toContain("raw abort reason");
    expect(fixture.discovery.leaseHeld).toBe(false);
  });

  test("maps hostile spawn failures without inspecting or exposing them", async () => {
    const fixture = await setup();
    const trap = () => {
      throw new Error("trap read");
    };
    fixture.spawner.failure = new Proxy(Object.create(null) as object, {
      get: trap,
      getPrototypeOf: trap,
    });

    await expect(fixture.launcher.connect()).rejects.toMatchObject({ code: "SPAWN_FAILED" });
  });
});
