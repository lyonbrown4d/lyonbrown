import { AgentClientError } from "@lyonbrown/agent-client";
import { deriveScope } from "@lyonbrown/local-control-plane";
import { describe, expect, test } from "vitest";
import { createAgentLauncher } from "../src/launcher.ts";
import type { CreateAgentLauncherOptions, DiscoveryClient } from "../src/types.ts";
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
} from "./support/fixtures.ts";

function options(
  discovery = new FakeDiscovery(),
  clients = new FakeClientFactory(),
  provider = nativeProvider(),
): CreateAgentLauncherOptions {
  const clock = new ManualClock();
  return {
    nativeProvider: provider,
    workspace: WORKSPACE,
    effectiveConfigFingerprint: CONFIG,
    ports: {
      discovery,
      clients,
      spawner: new FakeSpawner(discovery),
      clock,
      sleeper: new AdvancingSleeper(clock),
    },
    deadlines: { startupMs: 20, pollIntervalMs: 5 },
    client: { name: "agent-test", version: "0.0.0" },
  };
}

describe("validated configuration", () => {
  test.each([
    ["workspace", { workspace: " " }],
    ["config", { effectiveConfigFingerprint: "bad\0config" }],
    ["client name", { client: { name: "", version: "0.0.0" } }],
    ["client version", { client: { name: "agent-test", version: " " } }],
    ["startup deadline", { deadlines: { startupMs: 0, pollIntervalMs: 5 } }],
    ["poll deadline", { deadlines: { startupMs: 20, pollIntervalMs: 1.5 } }],
  ])("rejects invalid %s", (_label, override) => {
    expect(() => createAgentLauncher({ ...options(), ...override })).toThrow(
      expect.objectContaining({ code: "INVALID_CONFIGURATION" }),
    );
  });

  test("accepts defaults and caps a long poll interval", async () => {
    const configured = options();
    const launcher = createAgentLauncher({
      nativeProvider: configured.nativeProvider,
      workspace: configured.workspace,
      effectiveConfigFingerprint: configured.effectiveConfigFingerprint,
      ports: configured.ports,
      client: configured.client,
    });
    await expect(launcher.connect({ autoStart: false })).rejects.toMatchObject({
      code: "AUTO_START_DISABLED",
    });
  });
});

describe("client and epoch failure handling", () => {
  test.each(["DISCONNECTED", "TIMEOUT", "STALE_SERVER_EPOCH", "CANCELLED"] as const)(
    "does not reuse %s client failures",
    async (code) => {
      const provider = nativeProvider();
      const scope = await deriveScope(WORKSPACE, CONFIG, provider);
      const discovery = new FakeDiscovery();
      discovery.candidate = new Candidate("epoch-a", scope.scopeId);
      const clients = new FakeClientFactory();
      clients.handshakeFailure = new AgentClientError("unavailable", code, "private transport");
      const configured = options(discovery, clients, provider);

      await expect(
        createAgentLauncher(configured).connect({ autoStart: false }),
      ).rejects.toMatchObject({ code: "AUTO_START_DISABLED" });
      expect(clients.clients[0]?.closed).toBe(true);
    },
  );

  test("rejects a handshake scope that differs from its authenticated candidate", async () => {
    const provider = nativeProvider();
    const scope = await deriveScope(WORKSPACE, CONFIG, provider);
    const discovery = new FakeDiscovery();
    discovery.candidate = new Candidate("epoch-a", scope.scopeId);
    const clients = new FakeClientFactory();
    const original = clients.create.bind(clients);
    clients.create = (candidate, deadlines) => {
      const client = original(candidate, deadlines);
      client.handshake = () =>
        Promise.resolve({
          protocolVersion: 1,
          serverVersion: "test",
          serverInstanceId: "epoch-a",
          capabilities: [],
          scopeId: "f".repeat(64),
        });
      return client;
    };
    const configured = options(discovery, clients, provider);

    await expect(createAgentLauncher(configured).connect()).rejects.toMatchObject({
      code: "SCOPE_MISMATCH",
    });
  });

  test("discards a snapshot when its epoch disappears before confirmation", async () => {
    const provider = nativeProvider();
    const scope = await deriveScope(WORKSPACE, CONFIG, provider);
    const discovery = new FakeDiscovery();
    discovery.candidate = new Candidate("epoch-a", scope.scopeId);
    const clients = new FakeClientFactory();
    clients.onSnapshot = () => {
      discovery.candidate = null;
    };
    const configured = options(discovery, clients, provider);

    await expect(
      createAgentLauncher(configured).connect({ autoStart: false }),
    ).rejects.toMatchObject({ code: "AUTO_START_DISABLED" });
    expect(clients.clients[0]?.closed).toBe(true);
  });

  test("fails closed when repeated epoch changes never stabilize", async () => {
    const provider = nativeProvider();
    const scope = await deriveScope(WORKSPACE, CONFIG, provider);
    let epoch = 0;
    const discovery: DiscoveryClient = {
      read: () => Promise.resolve(new Candidate(`epoch-${(epoch++).toString()}`, scope.scopeId)),
      tryAcquireStartupLease: () => Promise.resolve(null),
    };
    const configured = options(new FakeDiscovery(), new FakeClientFactory(), provider);
    const epochOptions = {
      ...configured,
      ports: { ...configured.ports, discovery },
    };

    await expect(createAgentLauncher(epochOptions).connect()).rejects.toMatchObject({
      code: "EPOCH_UNSTABLE",
    });
  });
});
