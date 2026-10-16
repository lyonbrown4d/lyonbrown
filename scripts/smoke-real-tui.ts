import { spawn, type ChildProcess } from "node:child_process";
import { access, mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

import { createAgentClient, type AgentClient } from "@lyonbrown/agent-client";
import { createControlPlaneNativeProvider } from "@lyonbrown/control-plane-native";
import {
  bindDiscoveredEpoch,
  deriveScopedDaemonPaths,
  openSecureRunDirectory,
} from "@lyonbrown/local-control-plane";
import { config as loadEnvironment } from "dotenv";
import { spawn as spawnPty, type IPty } from "node-pty";
import { $, usePowerShell } from "zx";

import {
  TerminalTranscript,
  inspectTuiRender,
  requireTuiRender,
} from "@lyonbrown/workspace/internal/real-tui-blackbox";
import {
  appendBounded,
  assertDescriptor,
  childEnvironment,
  processExit,
  ptyExit,
  removeOwnedRunDirectory,
  requiredAbsoluteEnvironmentPath,
  stopProcess,
  waitForDiscovery,
  waitUntil,
  withTimeout,
} from "@lyonbrown/workspace/internal/real-tui-blackbox-process";
import { configureZxPlatform } from "@lyonbrown/workspace/internal/zx-platform";

const CONFIG_FINGERPRINT = "production-default";
const CAPTURE_LIMIT_BYTES = 2 * 1024 * 1024;
const STARTUP_TIMEOUT_MS = 120_000;
const TUI_TIMEOUT_MS = 30_000;
const EXIT_TIMEOUT_MS = 10_000;
const require = createRequire(import.meta.url);

async function main(): Promise<void> {
  if (!["win32", "darwin", "linux"].includes(process.platform)) {
    throw new Error("real TUI smoke does not support this platform");
  }
  configureZxPlatform(process.platform, usePowerShell);
  loadEnvironment({ path: resolve(".env.local"), override: false, quiet: true });
  const workspace = resolve(process.cwd());
  const scratchRoot = requiredAbsoluteEnvironmentPath(process.env, "LYONBROWN_BLACKBOX_ROOT");
  requiredAbsoluteEnvironmentPath(process.env, "LYONBROWN_DECISION_MODEL_ROOT");
  await $({ cwd: workspace })`pnpm exec moon run agentd:build agent-cli:build`;
  console.log("REAL_TUI_STAGE build_ready");

  await mkdir(scratchRoot, { recursive: true });
  const runRoot = await mkdtemp(join(scratchRoot, "run-"));
  const homeDirectory = join(runRoot, "home");
  await mkdir(join(homeDirectory, ".lyonbrown"), { recursive: true });
  await writeFile(
    join(homeDirectory, ".lyonbrown", "client-preferences.json"),
    `${JSON.stringify({ locale: "en-US", themeId: "carbon-mint" }, null, 2)}\n`,
    { encoding: "utf8", mode: 0o600 },
  );

  const environment = childEnvironment(process.env, homeDirectory);
  // Keep the chat setup flow isolated while reusing the explicitly configured decision cache.
  // Native storage requires both the model directory and its immediate parent to be private.
  environment.LYONBROWN_LOCAL_MODEL_ROOT = join(runRoot, "model-storage", "chat");
  const nativeProvider = createControlPlaneNativeProvider();
  let daemon: ChildProcess | undefined;
  let client: AgentClient | undefined;
  let tui: IPty | undefined;
  let transcript: TerminalTranscript | undefined;
  let daemonOutput = "";
  let report: Readonly<Record<string, unknown>> | undefined;
  try {
    const paths = await deriveScopedDaemonPaths(
      homeDirectory,
      workspace,
      CONFIG_FINGERPRINT,
      nativeProvider,
    );
    const agentdEntry = require.resolve("@lyonbrown/agentd/daemon");
    const cliEntry = resolve(workspace, "apps", "agent-cli", "dist", "main.js");
    await Promise.all([access(agentdEntry), access(cliEntry)]);
    daemon = spawn(
      process.execPath,
      [
        agentdEntry,
        "--workspace",
        paths.canonicalWorkspace,
        "--effective-config-fingerprint",
        CONFIG_FINGERPRINT,
        "--scope-id",
        paths.scopeId,
      ],
      {
        cwd: paths.canonicalWorkspace,
        env: environment,
        shell: false,
        windowsHide: true,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    daemon.stdout?.on("data", (chunk: Buffer) => {
      daemonOutput = appendBounded(daemonOutput, chunk.toString("utf8"));
    });
    daemon.stderr?.on("data", (chunk: Buffer) => {
      daemonOutput = appendBounded(daemonOutput, chunk.toString("utf8"));
    });
    const daemonExited = processExit(daemon);
    const descriptor = await waitForDiscovery(
      () => openSecureRunDirectory(paths.runDirectory, nativeProvider),
      daemon,
      daemonExited,
      STARTUP_TIMEOUT_MS,
    );
    assertDescriptor(descriptor, daemon, paths.scopeId);
    console.log("REAL_TUI_STAGE daemon_discovered");

    client = createAgentClient({
      binding: bindDiscoveredEpoch(descriptor),
      deadlines: { unaryMs: 10_000, commandAttempts: 1, retryBackoffMs: 1 },
    });
    const health = await client.health();
    const handshake = await client.handshake({
      clientName: "real-tui-blackbox",
      clientVersion: "0.0.0",
    });
    const snapshot = await client.getSnapshot();
    if (handshake.serverInstanceId !== descriptor.serverInstanceId) {
      throw new Error("gRPC handshake did not bind the discovered daemon epoch");
    }
    if (handshake.scopeId !== paths.scopeId) throw new Error("gRPC handshake scope mismatch");
    console.log("REAL_TUI_STAGE grpc_ready");

    const activeTranscript = new TerminalTranscript(CAPTURE_LIMIT_BYTES);
    transcript = activeTranscript;
    tui = spawnPty(process.execPath, [cliEntry], {
      name: "xterm-256color",
      cols: 120,
      rows: 30,
      cwd: paths.canonicalWorkspace,
      env: environment,
    });
    const tuiExited = ptyExit(tui);
    tui.onData((data) => {
      activeTranscript.append(data);
    });
    await waitUntil(
      () => {
        const evidence = inspectTuiRender(activeTranscript);
        return evidence.ansiRendered && evidence.chromeRendered && evidence.grpcReadyRendered;
      },
      TUI_TIMEOUT_MS,
      "TUI did not render the authenticated gRPC ready state",
    );
    await waitUntil(
      () => inspectTuiRender(activeTranscript).modelSetupRendered,
      TUI_TIMEOUT_MS,
      "TUI did not render daemon-projected model setup",
    );
    const render = requireTuiRender(activeTranscript);
    console.log("REAL_TUI_STAGE tui_rendered");
    const decisionRender = await exerciseDecisionTui(tui, activeTranscript);
    const decisionSnapshot = await client.getSnapshot();
    if (decisionSnapshot.decisionModel?.phase !== "configured")
      throw new Error("decision weights are not verified and configured");
    tui.write("q");
    const tuiResult = await withTimeout(tuiExited, EXIT_TIMEOUT_MS, "TUI did not exit cleanly");
    if (tuiResult.exitCode !== 0)
      throw new Error(`TUI exited with code ${String(tuiResult.exitCode)}`);
    tui = undefined;
    console.log("REAL_TUI_STAGE tui_exited");

    report = Object.freeze({
      marker: "REAL_AGENTD_GRPC_TUI_SMOKE_OK",
      scratchRoot,
      decisionModelRoot: process.env.LYONBROWN_DECISION_MODEL_ROOT,
      daemonPid: descriptor.pid,
      endpoint: descriptor.endpoint,
      health: health.status,
      serverInstanceId: handshake.serverInstanceId,
      scopeId: handshake.scopeId,
      snapshotPhase: snapshot.phase,
      snapshotRevision: snapshot.revision.toString(),
      render,
      decisionRender,
      decisionModelPhase: decisionSnapshot.decisionModel.phase,
      tuiExitCode: tuiResult.exitCode,
    });
  } catch (error: unknown) {
    const failedSnapshot = await client?.getSnapshot().catch(() => undefined);
    if (failedSnapshot !== undefined)
      console.error(
        JSON.stringify({
          stage: "failed_snapshot",
          chatModelPhase: failedSnapshot.modelSetup?.phase,
          chatModelFailureCode: failedSnapshot.modelSetup?.failure?.code,
          decisionModelPhase: failedSnapshot.decisionModel?.phase,
          decisionModelFailureCode: failedSnapshot.decisionModel?.failure?.code,
        }),
      );
    const details = daemonOutput.trim();
    if (details.length > 0) console.error(`agentd output:\n${details}`);
    const terminalTail = transcript?.plain.slice(-4_096).trim();
    if (terminalTail !== undefined && terminalTail.length > 0) {
      console.error(`TUI transcript tail:\n${terminalTail}`);
    }
    throw error;
  } finally {
    tui?.kill();
    await client?.close().catch(() => undefined);
    await stopProcess(daemon, EXIT_TIMEOUT_MS);
    await removeOwnedRunDirectory(scratchRoot, runRoot);
  }
  console.log(JSON.stringify(report));
}

async function exerciseDecisionTui(
  tui: IPty,
  transcript: TerminalTranscript,
): Promise<Readonly<Record<string, boolean>>> {
  tui.write("\u0010");
  await waitUntil(
    () => transcript.plain.includes("Command palette"),
    TUI_TIMEOUT_MS,
    "command palette did not open during chat setup",
  );
  // Individual keys preserve ordering in the terminal input parser; this is a bounded UI sequence.
  for (const character of "decision") {
    tui.write(character);
    await delay(40);
  }
  await waitUntil(
    () => transcript.plain.includes("Search: decision"),
    TUI_TIMEOUT_MS,
    "decision command search did not render",
  );
  tui.write("\r");
  await waitUntil(
    () => transcript.plain.includes("Installed · loads on first use"),
    STARTUP_TIMEOUT_MS,
    "verified decision model status did not render in English",
  );
  const beforeChinese = transcript.plain.length;
  tui.write("\u001bOQ");
  await waitUntil(
    () => transcript.plain.slice(beforeChinese).includes("已安装 · 首次使用时加载"),
    TUI_TIMEOUT_MS,
    "decision model status did not render in Chinese",
  );
  tui.write("\u001b");
  await delay(150);
  return Object.freeze({
    openedDuringChatSetup: true,
    englishConfiguredRendered: true,
    chineseConfiguredRendered: true,
  });
}

const entry = process.argv[1] === undefined ? undefined : pathToFileURL(process.argv[1]).href;
if (entry === import.meta.url) {
  void main().then(
    () => {
      process.stdout.write("", () => process.exit(0));
    },
    (error: unknown) => {
      process.stderr.write(`${describeError(error)}\n`, () => process.exit(1));
    },
  );
}

function describeError(error: unknown): string {
  if (!(error instanceof Error)) return String(error);
  const fields: string[] = [error.message];
  if ("code" in error && typeof error.code === "string") fields.push(`code=${error.code}`);
  if (
    "details" in error &&
    typeof error.details === "object" &&
    error.details !== null &&
    "operation" in error.details &&
    typeof error.details.operation === "string"
  ) {
    fields.push(`operation=${error.details.operation}`);
  }
  return fields.join(" ");
}
