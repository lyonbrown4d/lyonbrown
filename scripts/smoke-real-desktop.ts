import assert from "node:assert/strict";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdir, mkdtemp } from "node:fs/promises";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";
import { config as loadEnvironment } from "dotenv";
import { _electron, type ElectronApplication } from "playwright";
import { resolveConfig } from "electron-vite";
import { createServer, type ViteDevServer } from "vite";
import { $, retry, usePowerShell } from "zx";
import { createAgentClient, type AgentClient } from "@lyonbrown/agent-client";
import { createControlPlaneNativeProvider } from "@lyonbrown/control-plane-native";
import {
  bindDiscoveredEpoch,
  deriveScopedDaemonPaths,
  openSecureRunDirectory,
} from "@lyonbrown/local-control-plane";
import {
  assertDescriptor,
  childEnvironment,
  processExit,
  removeOwnedRunDirectory,
  requiredAbsoluteEnvironmentPath,
  stopProcess,
  waitForDiscovery,
} from "@lyonbrown/workspace/internal/real-tui-blackbox-process";
import { configureZxPlatform } from "@lyonbrown/workspace/internal/zx-platform";

interface ObservedBridge {
  getState(): Promise<{
    status: string;
    source: string | null;
    revision: string | null;
    canSend: boolean;
    phase: string;
  }>;
  act(action: {
    type: "send";
    text: string;
    token: string;
  }): Promise<{ ok: boolean; error: string | null }>;
}
async function main(): Promise<void> {
  configureZxPlatform(process.platform, usePowerShell);
  loadEnvironment({ path: resolve(".env.local"), quiet: true, override: false });
  const root = process.cwd();
  const scratch = requiredAbsoluteEnvironmentPath(process.env, "LYONBROWN_BLACKBOX_ROOT");
  requiredAbsoluteEnvironmentPath(process.env, "LYONBROWN_LOCAL_MODEL_ROOT");
  await $({ cwd: root })`pnpm exec moon run agent-desktop:build`;
  await mkdir(scratch, { recursive: true });
  const runRoot = await mkdtemp(join(scratch, "desktop-"));
  const homeDirectory = join(runRoot, "home");
  await mkdir(homeDirectory);
  const env = childEnvironment(process.env, homeDirectory);
  env.LYONBROWN_DESKTOP_WORKSPACE = root;
  env.LYONBROWN_DESKTOP_HOME = homeDirectory;
  env.LYONBROWN_DESKTOP_NODE_EXECUTABLE = process.execPath;
  env.LYONBROWN_DESKTOP_AUTOSTART = "false";
  delete env.ELECTRON_RENDERER_URL;
  const nativeProvider = createControlPlaneNativeProvider();
  const require = createRequire(import.meta.url);
  const desktopRequire = createRequire(resolve(root, "apps/agent-desktop/package.json"));
  let daemon: ChildProcess | undefined;
  let client: AgentClient | undefined;
  let desktop: ElectronApplication | undefined;
  let rendererServer: ViteDevServer | undefined;
  const development = process.argv.includes("--dev");
  try {
    const paths = await deriveScopedDaemonPaths(
      homeDirectory,
      root,
      "production-default",
      nativeProvider,
    );
    daemon = spawn(
      process.execPath,
      [
        require.resolve("@lyonbrown/agentd/daemon"),
        "--workspace",
        paths.canonicalWorkspace,
        "--effective-config-fingerprint",
        "production-default",
        "--scope-id",
        paths.scopeId,
      ],
      { cwd: root, env, shell: false, windowsHide: true, stdio: "ignore" },
    );
    const descriptor = await waitForDiscovery(
      () => openSecureRunDirectory(paths.runDirectory, nativeProvider),
      daemon,
      processExit(daemon),
      120_000,
    );
    assertDescriptor(descriptor, daemon, paths.scopeId);
    client = createAgentClient({ binding: bindDiscoveredEpoch(descriptor) });
    assert.equal((await client.health()).status, "serving");
    await client.handshake({ clientName: "desktop-blackbox", clientVersion: "0.0.0" });
    const executable: unknown = desktopRequire("electron");
    assert.equal(typeof executable, "string");
    if (typeof executable !== "string") throw new Error("ELECTRON_NOT_INSTALLED");
    if (development) {
      const resolved = await resolveConfig(
        {
          root: resolve(root, "apps/agent-desktop"),
          configFile: resolve(root, "apps/agent-desktop/electron.vite.config.ts"),
        },
        "serve",
      );
      rendererServer = await createServer({
        ...resolved.config?.renderer,
        configFile: false,
        root: resolve(root, "apps/agent-desktop/src/renderer"),
        server: { host: "127.0.0.1", port: 0 },
      });
      await rendererServer.listen();
      const url = rendererServer.resolvedUrls?.local[0];
      assert.ok(url);
      env.ELECTRON_RENDERER_URL = url;
    }
    desktop = await _electron.launch({
      executablePath: executable,
      args: [
        resolve(root, "apps/agent-desktop/dist/main/index.js"),
        "--user-data-dir=" + join(runRoot, "browser-data"),
      ],
      cwd: root,
      env: Object.fromEntries(
        Object.entries(env).filter((entry): entry is [string, string] => entry[1] !== undefined),
      ),
      chromiumSandbox: true,
      timeout: 30_000,
    });
    const page = await desktop.firstWindow();
    page.setDefaultTimeout(30_000);
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page
      .getByRole("status")
      .filter({ hasText: /^connected$/u })
      .waitFor();
    await page.getByLabel("Message", { exact: true }).fill("", { timeout: 120_000 });
    const initial = await page.evaluate(async () =>
      (globalThis as unknown as { lyonbrown: ObservedBridge }).lyonbrown.getState(),
    );
    assert.equal(initial.source, "reused");
    const exposed = await page.evaluate(() => ({
      keys: Object.keys((globalThis as unknown as { lyonbrown: ObservedBridge }).lyonbrown).sort(),
      node: "require" in globalThis || "process" in globalThis,
    }));
    assert.deepEqual(exposed.keys, ["act", "getState", "onState", "reconnect"]);
    assert.equal(exposed.node, false);
    const security = { rendererNodeApi: exposed.node, restrictedBridge: true };
    const stale = await page.evaluate(async () =>
      (globalThis as unknown as { lyonbrown: ObservedBridge }).lyonbrown.act({
        type: "send",
        text: "must not execute",
        token: "stale:0",
      }),
    );
    assert.equal(stale.error, "STALE_STATE");
    const prompt = "Reply briefly with DESKTOP_OK. Do not use any tools.";
    await page.getByLabel("Message", { exact: true }).fill(prompt);
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await page.getByText(prompt, { exact: true }).waitFor();
    await retry(240, "500ms", async () => {
      const state = await page.evaluate(async () =>
        (globalThis as unknown as { lyonbrown: ObservedBridge }).lyonbrown.getState(),
      );
      assert.equal(state.phase, "completed");
      assert.equal(state.canSend, true);
    });
    const snapshot = await client.getSnapshot();
    const turn = snapshot.turns.at(-1);
    assert.ok(turn);
    assert.equal(turn.userText, prompt);
    assert.equal(turn.status, "completed");
    assert.ok(turn.assistantText.trim().length > 0);
    await page.getByText(turn.assistantText, { exact: true }).waitFor();
    const screenshotRoot = resolve(root, "output/playwright");
    await mkdir(screenshotRoot, { recursive: true });
    await page.screenshot({ path: join(screenshotRoot, "desktop-en.png") });
    await page.getByRole("button", { name: "中文", exact: true }).click();
    await page.getByRole("heading", { name: "会话", exact: true }).waitFor();
    await page.screenshot({ path: join(screenshotRoot, "desktop-zh.png") });
    await stopProcess(daemon, 10_000);
    await page.getByRole("button", { name: "重新连接", exact: true }).waitFor();
    assert.equal(await page.getByRole("button", { name: "发送", exact: true }).isDisabled(), true);
    assert.deepEqual(errors, []);
    console.log(
      JSON.stringify({
        marker: "REAL_AGENTD_GRPC_DESKTOP_SMOKE_OK",
        mode: development ? "development" : "built",
        daemonPid: descriptor.pid,
        source: initial.source,
        turnStatus: turn.status,
        security,
        languages: ["en", "zh"],
        disconnectedReadOnly: true,
        screenshots: screenshotRoot,
      }),
    );
  } finally {
    await desktop?.close();
    await rendererServer?.close();
    await client?.close();
    await stopProcess(daemon, 10_000);
    await removeOwnedRunDirectory(scratch, runRoot);
  }
}
void main().catch((error: unknown) => {
  console.error(error);
  process.exitCode = 1;
});
