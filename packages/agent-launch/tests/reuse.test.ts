import { AgentClientError } from "@lyonbrown/agent-client";
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

async function setup() {
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
    deadlines: { startupMs: 100, pollIntervalMs: 5 },
    client: { name: "agent-test", version: "0.0.0" },
  });
  return { scope, discovery, clients, spawner, launcher };
}

describe("authenticated daemon reuse", () => {
  test("reuses only an authenticated candidate in the derived configuration scope", async () => {
    const fixture = await setup();
    fixture.discovery.candidate = new Candidate("epoch-a", fixture.scope.scopeId);

    const connection = await fixture.launcher.connect();

    expect(connection.serverInstanceId).toBe("epoch-a");
    expect(connection.scopeId).toBe(fixture.scope.scopeId);
    expect(connection.snapshot.revision).toBe(1n);
    expect(connection.source).toBe("reused");
    expect(fixture.spawner.calls).toHaveLength(0);
    expect(fixture.discovery.lockAttempts).toBe(0);
  });

  test("fails closed on authentication rejection without spawning or exposing its cause", async () => {
    const fixture = await setup();
    fixture.discovery.candidate = new Candidate("epoch-a", fixture.scope.scopeId);
    fixture.clients.handshakeFailure = new AgentClientError(
      "authentication",
      "AUTHENTICATION_FAILED",
      "raw token secret",
    );

    const error = await fixture.launcher.connect().catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(AgentLaunchError);
    expect(error).toMatchObject({ code: "AUTHENTICATION_FAILED" });
    expect(String(error)).not.toContain("secret");
    expect(error).not.toHaveProperty("cause");
    expect(fixture.spawner.calls).toHaveLength(0);
  });

  test("rejects a responsive daemon from another configuration scope", async () => {
    const fixture = await setup();
    fixture.discovery.candidate = new Candidate("epoch-a", "f".repeat(64));

    await expect(fixture.launcher.connect()).rejects.toMatchObject({ code: "SCOPE_MISMATCH" });
    expect(fixture.spawner.calls).toHaveLength(0);
  });

  test("rejects a handshake whose server instance differs from discovery", async () => {
    const fixture = await setup();
    fixture.discovery.candidate = new Candidate("epoch-a", fixture.scope.scopeId);
    const originalCreate = fixture.clients.create.bind(fixture.clients);
    fixture.clients.create = (candidate, deadlines) => {
      const client = originalCreate(candidate, deadlines);
      (client as unknown as { candidate: Candidate }).candidate = new Candidate(
        "epoch-other",
        fixture.scope.scopeId,
      );
      return client;
    };

    await expect(fixture.launcher.connect()).rejects.toMatchObject({
      code: "SERVER_INSTANCE_MISMATCH",
    });
  });

  test("rebinds and resynchronizes when discovery changes epoch without replaying commands", async () => {
    const fixture = await setup();
    const first = new Candidate("epoch-a", fixture.scope.scopeId);
    const second = new Candidate("epoch-b", fixture.scope.scopeId);
    fixture.discovery.candidate = first;
    let switched = false;
    fixture.clients.onSnapshot = () => {
      if (!switched) fixture.discovery.candidate = second;
      switched = true;
    };
    fixture.clients.revisionFor = (candidate) => (candidate === second ? 2n : 1n);

    const connection = await fixture.launcher.connect();

    expect(connection.serverInstanceId).toBe("epoch-b");
    expect(connection.snapshot.revision).toBe(2n);
    expect(fixture.clients.replacements).toEqual([second]);
    expect(fixture.clients.clients).toHaveLength(1);
    expect(fixture.clients.clients[0]?.enqueueCalls).toBe(0);
  });
});
