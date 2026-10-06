import { EventEmitter } from "node:events";
import { describe, expect, test } from "vitest";
import {
  NodeProcessSpawner,
  type SpawnProcess,
  type SpawnedAgentdProcess,
} from "@lyonbrown/agent-launch/internal/production-spawner";

class FakeChild extends EventEmitter implements SpawnedAgentdProcess {
  public killCalls = 0;
  public unrefCalls = 0;

  public kill(): boolean {
    this.killCalls += 1;
    return true;
  }

  public unref(): void {
    this.unrefCalls += 1;
  }
}

function request() {
  return {
    canonicalWorkspace: process.platform === "win32" ? "C:\\repo & whoami" : "/repo & whoami",
    effectiveConfigFingerprint: "configuration;echo unsafe",
    scopeId: "a".repeat(64),
  };
}

describe("production process spawner", () => {
  test("starts asynchronously with explicit arguments and no shell", async () => {
    const child = new FakeChild();
    const calls: Parameters<SpawnProcess>[] = [];
    const spawn: SpawnProcess = (...input) => {
      calls.push(input);
      return child;
    };
    const spawner = new NodeProcessSpawner(
      {
        command: process.execPath,
        arguments: ["agentd.js", "--foreground"],
        workingDirectory: process.cwd(),
      },
      spawn,
    );

    const attempt = spawner.begin(request());
    expect(calls).toHaveLength(0);

    await Promise.resolve();
    expect(calls).toEqual([
      [
        process.execPath,
        [
          "agentd.js",
          "--foreground",
          "--workspace",
          request().canonicalWorkspace,
          "--effective-config-fingerprint",
          request().effectiveConfigFingerprint,
          "--scope-id",
          request().scopeId,
        ],
        {
          cwd: process.cwd(),
          detached: true,
          shell: false,
          stdio: "ignore",
          windowsHide: true,
        },
      ],
    ]);
    expect(child.unrefCalls).toBe(1);

    child.emit("exit", 17, null);
    child.emit("close", 17, null);
    await expect(attempt.exited).resolves.toEqual({ exitCode: 17 });
    await expect(attempt.retired).resolves.toBeUndefined();
  });

  test("retires an abandoned attempt before it can start", async () => {
    const child = new FakeChild();
    let spawnCalls = 0;
    const spawn: SpawnProcess = () => {
      spawnCalls += 1;
      return child;
    };
    const spawner = new NodeProcessSpawner({ command: process.execPath }, spawn);

    const attempt = spawner.begin(request());
    await attempt.abandon(new AbortController().signal);
    await Promise.resolve();

    expect(spawnCalls).toBe(0);
    expect(child.killCalls).toBe(0);
    await expect(attempt.exited).resolves.toEqual({ exitCode: null });
    await expect(attempt.retired).resolves.toBeUndefined();
  });

  test("terminates only its owned child and retires after close", async () => {
    const child = new FakeChild();
    const spawner = new NodeProcessSpawner({ command: process.execPath }, () => child);
    const attempt = spawner.begin(request());
    await Promise.resolve();

    const abandoning = attempt.abandon(new AbortController().signal);
    expect(child.killCalls).toBe(1);
    child.emit("exit", null, "SIGTERM");
    child.emit("close", null, "SIGTERM");

    await expect(abandoning).resolves.toBeUndefined();
    await expect(attempt.exited).resolves.toEqual({ exitCode: null });
    await expect(attempt.retired).resolves.toBeUndefined();
  });

  test("reports a spawn failure as terminal without leaking the underlying error", async () => {
    const spawner = new NodeProcessSpawner({ command: process.execPath }, () => {
      throw new Error("private operating-system failure");
    });

    const attempt = spawner.begin(request());

    await expect(attempt.exited).resolves.toEqual({ exitCode: null });
    await expect(attempt.retired).resolves.toBeUndefined();
  });

  test("retires when the child reports an error before spawn succeeds", async () => {
    const child = new FakeChild();
    const spawner = new NodeProcessSpawner({ command: process.execPath }, () => child);
    const attempt = spawner.begin(request());
    await Promise.resolve();

    child.emit("error", new Error("private pre-spawn failure"));

    await expect(attempt.exited).resolves.toEqual({ exitCode: null });
    await expect(attempt.retired).resolves.toBeUndefined();
  });

  test("keeps a spawned child owned after kill emits an error until exit is confirmed", async () => {
    const child = new FakeChild();
    const spawner = new NodeProcessSpawner({ command: process.execPath }, () => child);
    const attempt = spawner.begin(request());
    await Promise.resolve();
    child.emit("spawn");

    let exited = false;
    let retired = false;
    void attempt.exited.then(() => {
      exited = true;
    });
    void attempt.retired.then(() => {
      retired = true;
    });
    const controller = new AbortController();
    const abandoning = attempt.abandon(controller.signal);
    child.emit("error", new Error("private post-spawn kill failure"));
    await Promise.resolve();

    expect(child.killCalls).toBe(1);
    expect(exited).toBe(false);
    expect(retired).toBe(false);

    controller.abort();
    await expect(abandoning).rejects.toThrow("spawn abandonment was cancelled");
    expect(retired).toBe(false);

    child.emit("exit", null, "SIGTERM");
    child.emit("close", null, "SIGTERM");
    await expect(attempt.exited).resolves.toEqual({ exitCode: null });
    await expect(attempt.retired).resolves.toBeUndefined();
  });
});
