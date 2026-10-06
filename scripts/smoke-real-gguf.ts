import { mkdtemp, mkdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import type { AgentClient, RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import type { createProductionAgentLauncher } from "@lyonbrown/agent-launch/composition";
import type { startProductionAgentd } from "@lyonbrown/agentd/composition";
import type { RunningProductionAgentd } from "@lyonbrown/agentd/composition";
import type { NativeControlPlaneProvider } from "@lyonbrown/local-control-plane";
import { $, usePowerShell } from "zx";

import { verifyInstalledModelCache } from "@lyonbrown/workspace/internal/smoke-real-gguf-cache";
import {
  runRealGgufSmokeFlow,
  type RealGgufSmokePort,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-flow";
import {
  safeSnapshot,
  SnapshotReporter,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-report";
import { configureZxPlatform } from "@lyonbrown/workspace/internal/zx-platform";
import {
  assertSafeTemporaryRoot,
  bounded,
  cleanupTemporaryAfterClose,
  parseRealGgufSmokeAuthorization,
  runAllAttemptCleanup,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-lib";

const FINGERPRINT = "real-gguf-release-smoke-v1";
const DEPENDENCY_BUILD_TIMEOUT_MS = 15 * 60 * 1_000;
const PHASE_TIMEOUT_MS = 120_000;
const DOWNLOAD_TIMEOUT_MS = 60 * 60_000;
const CLOSE_TIMEOUT_MS = 30_000;
const ACTIVE_CLOSE_TIMEOUT_MS = 15_000;
const EXPECTED_SHA256 = "228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f";

configureZxPlatform(process.platform, usePowerShell);

interface RuntimeModules {
  readonly createProductionAgentLauncher: typeof createProductionAgentLauncher;
  readonly startProductionAgentd: typeof startProductionAgentd;
}

class ProductionSmokePort implements RealGgufSmokePort {
  private server: RunningProductionAgentd | undefined;
  private client: AgentClient | undefined;
  private lastSnapshot: RuntimeSnapshotDto | undefined;
  private cleanupRemoved = false;
  private readonly reporter = new SnapshotReporter();

  public constructor(
    private readonly modules: RuntimeModules,
    private readonly nativeProvider: NativeControlPlaneProvider,
    private readonly homeDirectory: string,
    private readonly workspace: string,
    private readonly modelsDirectory: string,
    private readonly scopeId: string,
    private readonly cleanupRoot: string,
  ) {}

  public async start(): Promise<void> {
    this.reporter.stage("server-start");
    this.server = await this.modules.startProductionAgentd({
      workspace: this.workspace,
      homeDirectory: this.homeDirectory,
      effectiveConfigFingerprint: FINGERPRINT,
      expectedScopeId: this.scopeId,
      nativeProvider: this.nativeProvider,
    });
    const launcher = this.modules.createProductionAgentLauncher({
      nativeProvider: this.nativeProvider,
      workspace: this.workspace,
      homeDirectory: this.homeDirectory,
      effectiveConfigFingerprint: FINGERPRINT,
      client: { name: "real-gguf-release-smoke", version: "1" },
      agentd: { command: process.execPath },
      deadlines: {
        startupMs: PHASE_TIMEOUT_MS,
        cleanupMs: CLOSE_TIMEOUT_MS,
        client: { unaryMs: PHASE_TIMEOUT_MS, commandAttempts: 1 },
      },
    });
    const connection = await launcher.connect({ autoStart: false });
    this.client = connection.client;
    this.lastSnapshot = connection.snapshot;
  }

  public async readSnapshot(): Promise<RuntimeSnapshotDto> {
    const snapshot = await this.requireClient().getSnapshot();
    this.lastSnapshot = snapshot;
    this.reporter.snapshot(snapshot);
    return snapshot;
  }

  public async startDownload(modelId: string): Promise<void> {
    this.reporter.stage("download-start");
    await this.enqueue({ type: "start-model-download", modelId });
  }

  public async cancelDownload(operationId: string): Promise<void> {
    this.reporter.stage("download-cancel");
    await this.enqueue({ type: "cancel-model-download", operationId });
  }

  public async startTurn(text: string): Promise<void> {
    this.reporter.stage("turn-start");
    await this.enqueue({ type: "start-turn", text });
  }

  public async cancelTurn(): Promise<void> {
    this.reporter.stage("turn-cancel");
    await this.enqueue({ type: "cancel-turn" });
  }

  public async closeServer(): Promise<void> {
    const client = this.client;
    const server = this.server;
    await runAllAttemptCleanup([
      ...(server === undefined
        ? []
        : [
            async () => {
              await server.close();
              if (this.server === server) this.server = undefined;
            },
          ]),
      ...(client === undefined
        ? []
        : [
            async () => {
              await client.close();
              if (this.client === client) this.client = undefined;
            },
          ]),
    ]);
  }

  public async restartServer(): Promise<void> {
    await this.start();
  }

  public verifyInstalledCache(expectedSha256: string): Promise<void> {
    return verifyInstalledModelCache(this.modelsDirectory, expectedSha256);
  }

  public cleanup(): Promise<void> {
    return cleanupTemporaryAfterClose(
      () => bounded("final-server-close", CLOSE_TIMEOUT_MS, this.closeServer()),
      () => bounded("temporary-directory-cleanup", CLOSE_TIMEOUT_MS, this.removeTemporaryRoot()),
    );
  }

  public safeLastSnapshot(): unknown {
    return safeSnapshot(this.lastSnapshot);
  }

  private requireClient(): AgentClient {
    if (this.client === undefined) throw new Error("REAL_GGUF_CLIENT_UNAVAILABLE");
    return this.client;
  }

  private async enqueue(command: Parameters<AgentClient["enqueue"]>[0]): Promise<void> {
    const receipt = await this.requireClient().enqueue(command);
    if (!acceptedReceipt(receipt)) throw new Error("REAL_GGUF_COMMAND_NOT_ACCEPTED");
  }

  private async removeTemporaryRoot(): Promise<void> {
    if (this.cleanupRemoved) return;
    await assertSafeTemporaryRoot(this.cleanupRoot);
    await rm(this.cleanupRoot, { recursive: true, force: true });
    this.cleanupRemoved = true;
  }
}

export async function main(
  arguments_: readonly string[] = process.argv.slice(2),
  environment: Readonly<Record<string, string | undefined>> = process.env,
): Promise<void> {
  let temporaryRoot: string | undefined;
  let port: ProductionSmokePort | undefined;
  let failure: unknown;
  try {
    parseRealGgufSmokeAuthorization(environment, arguments_);
    await buildRuntimeDependencies();
    const runtimeModules = await loadRuntimeModules();
    temporaryRoot = await mkdtemp(join(tmpdir(), "lyonbrown-real-gguf-"));
    const homeDirectory = join(temporaryRoot, "home");
    const workspace = join(temporaryRoot, "workspace");
    await Promise.all([mkdir(homeDirectory), mkdir(workspace)]);
    const providerModule = await import("@lyonbrown/control-plane-native");
    const controlPlane = await import("@lyonbrown/local-control-plane");
    const provider = providerModule.createControlPlaneNativeProvider();
    const paths = await controlPlane.deriveScopedDaemonPaths(
      homeDirectory,
      workspace,
      FINGERPRINT,
      provider,
    );
    port = new ProductionSmokePort(
      runtimeModules,
      provider,
      homeDirectory,
      workspace,
      join(paths.dataDirectory, "models"),
      paths.scopeId,
      temporaryRoot,
    );
    await port.start();
    await runRealGgufSmokeFlow(port, {
      phaseTimeoutMs: PHASE_TIMEOUT_MS,
      downloadTimeoutMs: DOWNLOAD_TIMEOUT_MS,
      pollIntervalMs: 100,
      cancelAfterBytes: 1024n * 1024n,
      closeTimeoutMs: ACTIVE_CLOSE_TIMEOUT_MS,
      expectedSha256: EXPECTED_SHA256,
    });
  } catch (error: unknown) {
    failure = error;
  }
  try {
    if (port !== undefined) await port.cleanup();
    else if (temporaryRoot !== undefined) await removeTemporaryRoot(temporaryRoot);
  } catch (cleanupError: unknown) {
    failure =
      failure === undefined
        ? cleanupError
        : new AggregateError([failure, cleanupError], "real GGUF smoke cleanup failed", {
            cause: failure,
          });
  }
  if (failure !== undefined) {
    process.stderr.write(
      `${JSON.stringify({
        code: failure instanceof Error ? failure.message : "REAL_GGUF_SMOKE_FAILED",
        snapshot: port?.safeLastSnapshot() ?? null,
      })}\n`,
    );
    throw failure instanceof Error ? failure : new Error("REAL_GGUF_SMOKE_FAILED");
  }
  process.stdout.write("REAL_GGUF_SMOKE_OK\n");
}

function acceptedReceipt(value: unknown): boolean {
  return (
    typeof value === "object" && value !== null && "accepted" in value && value.accepted === true
  );
}

async function buildRuntimeDependencies(): Promise<void> {
  await $({
    stdio: "inherit",
    timeout: DEPENDENCY_BUILD_TIMEOUT_MS,
  })`pnpm dependencies:build`;
}

async function removeTemporaryRoot(path: string): Promise<void> {
  await assertSafeTemporaryRoot(path);
  await rm(path, { recursive: true, force: true });
}

async function loadRuntimeModules(): Promise<RuntimeModules> {
  const [launcher, agentd] = await Promise.all([
    import("@lyonbrown/agent-launch/composition"),
    import("@lyonbrown/agentd/composition"),
  ]);
  return {
    createProductionAgentLauncher: launcher.createProductionAgentLauncher,
    startProductionAgentd: agentd.startProductionAgentd,
  };
}

const entryUrl = process.argv[1] === undefined ? undefined : pathToFileURL(process.argv[1]).href;
if (entryUrl === import.meta.url) {
  void main().catch(() => {
    process.exitCode = 1;
  });
}
