import { AgentClientError } from "@lyonbrown/agent-client";
import { deriveScope, type NativeControlPlaneProvider } from "@lyonbrown/local-control-plane";
import { describe, expect, test } from "vitest";
import { createAgentLauncher } from "../src/launcher.js";
import type { CreateAgentLauncherOptions } from "../src/types.js";
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
} from "./support/fixtures.js";

function options(
  discovery = new FakeDiscovery(),
  clients = new FakeClientFactory(),
  provider: NativeControlPlaneProvider = nativeProvider(),
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

function pending<T>(): Promise<T> {
  return new Promise<T>(() => undefined);
}

async function settleWithin<T>(promise: Promise<T>): Promise<T | "hung"> {
  return Promise.race([
    promise,
    new Promise<"hung">((resolve) => {
      setTimeout(() => {
        resolve("hung");
      }, 50);
    }),
  ]);
}

describe("safe adapter failures and bounded cleanup", () => {
  test("fails closed when the native provider is not a registered capability", async () => {
    const configured = options();
    const forged = {} as NativeControlPlaneProvider;
    const launcher = createAgentLauncher({ ...configured, nativeProvider: forged });

    await expect(launcher.connect()).rejects.toMatchObject({ code: "CONTROL_PLANE_FAILED" });
  });

  test("redacts discovery and startup lease failures", async () => {
    const discovery = new FakeDiscovery();
    discovery.readFailure = new Error("private descriptor path");
    await expect(createAgentLauncher(options(discovery)).connect()).rejects.toMatchObject({
      code: "CONTROL_PLANE_FAILED",
    });

    discovery.readFailure = undefined;
    discovery.lockFailure = new Error("private lock path");
    await expect(createAgentLauncher(options(discovery)).connect()).rejects.toMatchObject({
      code: "CONTROL_PLANE_FAILED",
    });
  });

  test("redacts timer and polling failures", async () => {
    const configured = options();
    const sleeper = configured.ports.sleeper as AdvancingSleeper;
    sleeper.failure = new Error("private timer failure");

    await expect(createAgentLauncher(configured).connect()).rejects.toMatchObject({
      code: "CLIENT_FAILED",
    });
  });

  test("closes a ready client when startup lease release fails", async () => {
    const provider = nativeProvider();
    const scope = await deriveScope(WORKSPACE, CONFIG, provider);
    const discovery = new FakeDiscovery();
    discovery.tryAcquireStartupLease = () =>
      Promise.resolve({ release: () => Promise.reject(new Error("private release path")) });
    const clients = new FakeClientFactory();
    const configured = options(discovery, clients, provider);
    (configured.ports.spawner as FakeSpawner).candidateToPublish = new Candidate(
      "epoch-ready",
      scope.scopeId,
    );

    await expect(createAgentLauncher(configured).connect()).rejects.toMatchObject({
      code: "CONTROL_PLANE_FAILED",
    });
    expect(clients.clients[0]?.closed).toBe(true);
  });

  test("bounds a startup lease release that never settles", async () => {
    const provider = nativeProvider();
    const scope = await deriveScope(WORKSPACE, CONFIG, provider);
    const discovery = new FakeDiscovery();
    discovery.tryAcquireStartupLease = () => Promise.resolve({ release: () => pending() });
    const configured = options(discovery, new FakeClientFactory(), provider);
    (configured.ports.sleeper as AdvancingSleeper).resolveDeadlineSleepCalls.add(10);
    (configured.ports.spawner as FakeSpawner).candidateToPublish = new Candidate(
      "epoch-ready",
      scope.scopeId,
    );

    const outcome = await settleWithin(
      createAgentLauncher(configured)
        .connect()
        .catch((error: unknown) => error),
    );

    expect(outcome).not.toBe("hung");
    expect(outcome).toMatchObject({ code: "CONTROL_PLANE_FAILED" });
  });

  test("bounds a client close that never settles", async () => {
    const discovery = new FakeDiscovery();
    const provider = nativeProvider();
    const scope = await deriveScope(WORKSPACE, CONFIG, provider);
    discovery.candidate = new Candidate("epoch-a", scope.scopeId);
    const clients = new FakeClientFactory();
    clients.handshakeFailure = new AgentClientError("unavailable", "DISCONNECTED", "private");
    const originalCreate = clients.create.bind(clients);
    clients.create = (candidate, deadlines) => {
      const client = originalCreate(candidate, deadlines);
      client.close = () => pending();
      return client;
    };
    const configured = options(discovery, clients, provider);
    (configured.ports.sleeper as AdvancingSleeper).resolveDeadlineSleepCalls.add(4);

    const outcome = await settleWithin(
      createAgentLauncher(configured)
        .connect({ autoStart: false })
        .catch((error: unknown) => error),
    );

    expect(outcome).not.toBe("hung");
    expect(outcome).toMatchObject({ code: "AUTO_START_DISABLED" });
  });

  test("redacts client construction and hostile client failures", async () => {
    const discovery = new FakeDiscovery();
    const provider = nativeProvider();
    const scope = await deriveScope(WORKSPACE, CONFIG, provider);
    discovery.candidate = new Candidate("epoch-a", scope.scopeId);
    const clients = new FakeClientFactory();
    clients.create = () => {
      throw new Error("private binding");
    };
    await expect(
      createAgentLauncher(options(discovery, clients, provider)).connect(),
    ).rejects.toMatchObject({ code: "CLIENT_FAILED" });

    const hostileClients = new FakeClientFactory();
    const trap = () => {
      throw new Error("private trap");
    };
    hostileClients.handshakeFailure = new Proxy(Object.create(null) as object, {
      getPrototypeOf: trap,
    });
    await expect(
      createAgentLauncher(options(discovery, hostileClients, provider)).connect(),
    ).rejects.toMatchObject({ code: "CLIENT_FAILED" });
  });

  test("guards a proxied AgentClientError code getter", async () => {
    const discovery = new FakeDiscovery();
    const provider = nativeProvider();
    const scope = await deriveScope(WORKSPACE, CONFIG, provider);
    discovery.candidate = new Candidate("epoch-a", scope.scopeId);
    const clients = new FakeClientFactory();
    const underlying = new AgentClientError("unavailable", "DISCONNECTED", "private transport");
    clients.handshakeFailure = new Proxy(underlying, {
      get(_target, property) {
        if (property === "code") throw new Error("raw proxy secret");
        return undefined;
      },
    });

    const error = await createAgentLauncher(options(discovery, clients, provider))
      .connect()
      .catch((caught: unknown) => caught);

    expect(error).toMatchObject({ code: "CLIENT_FAILED" });
    expect(String(error)).not.toContain("secret");
    expect(error).not.toHaveProperty("cause");
  });
});
