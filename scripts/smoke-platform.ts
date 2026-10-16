import assert from "node:assert/strict";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtemp, mkdir, realpath, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { createAgentClient, type AgentClient } from "@lyonbrown/agent-client";
import { createControlPlaneNativeProvider } from "@lyonbrown/control-plane-native";
import {
  bindDiscoveredEpoch,
  deriveScopedDaemonPaths,
  openSecureRunDirectory,
} from "@lyonbrown/local-control-plane";
import { $, usePowerShell } from "zx";

import {
  assertDescriptor,
  childEnvironment,
  processExit,
  stopProcess,
  waitForDiscovery,
} from "@lyonbrown/workspace/internal/real-tui-blackbox-process";
import { configureZxPlatform } from "@lyonbrown/workspace/internal/zx-platform";

configureZxPlatform(process.platform, usePowerShell);
const require = createRequire(import.meta.url);
const FINGERPRINT = "platform-smoke-v1";

async function main(): Promise<void> {
  await $({ stdio: "inherit" })`pnpm exec moon run agentd:build`;
  const scratch = await realpath(await mkdtemp(join(tmpdir(), "lyonbrown-platform-")));
  let daemon: ChildProcess | undefined;
  let client: AgentClient | undefined;
  try {
    const home = join(scratch, "home");
    const workspace = join(scratch, "workspace");
    await Promise.all([mkdir(home, { mode: 0o700 }), mkdir(workspace, { mode: 0o700 })]);
    const provider = createControlPlaneNativeProvider();
    const paths = await deriveScopedDaemonPaths(home, workspace, FINGERPRINT, provider);
    const environment = childEnvironment(process.env, home);
    environment.LYONBROWN_LOCAL_MODEL_ROOT = join(scratch, "models", "chat");
    environment.LYONBROWN_DECISION_MODEL_ROOT = join(scratch, "models", "decision");
    daemon = spawn(
      process.execPath,
      [
        require.resolve("@lyonbrown/agentd/daemon"),
        "--workspace",
        paths.canonicalWorkspace,
        "--effective-config-fingerprint",
        FINGERPRINT,
        "--scope-id",
        paths.scopeId,
      ],
      {
        cwd: workspace,
        env: environment,
        shell: false,
        windowsHide: true,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    // Keep untrusted child diagnostics private; pipe readers prevent backpressure.
    daemon.stdout?.resume();
    daemon.stderr?.resume();
    await new Promise<void>((resolve, reject) => {
      daemon!.once("spawn", resolve);
      daemon!.once("error", () => reject(new Error("platform daemon could not start")));
    });
    const exited = processExit(daemon);
    const descriptor = await waitForDiscovery(
      () => openSecureRunDirectory(paths.runDirectory, provider),
      daemon,
      exited,
      30_000,
    );
    assertDescriptor(descriptor, daemon, paths.scopeId);
    client = createAgentClient({ binding: bindDiscoveredEpoch(descriptor) });
    const handshake = await client.handshake({
      clientName: "platform-smoke",
      clientVersion: "0.0.0",
    });
    assert.equal(handshake.serverInstanceId, descriptor.serverInstanceId);
    assert.equal((await client.health()).status, "serving");
    const snapshot = await client.getSnapshot();
    assert.equal(snapshot.modelSetup?.downloadedBytes ?? 0n, 0n);
  } finally {
    const cleanup = await Promise.allSettled([client?.close(), stopProcess(daemon, 10_000)]);
    const failures = cleanup.filter((result) => result.status === "rejected");
    if (failures.length > 0) throw new Error("platform smoke cleanup failed");
    await rm(scratch, { recursive: true, force: true });
  }
  console.log(`PLATFORM_SMOKE_OK ${process.platform}-${process.arch}`);
}

void main().catch(() => {
  console.error("PLATFORM_SMOKE_FAILED");
  process.exitCode = 1;
});
