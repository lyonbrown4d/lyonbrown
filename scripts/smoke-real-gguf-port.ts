import { spawn, type ChildProcess } from "node:child_process";
import { readFile } from "node:fs/promises";
import { join } from "node:path";

import {
  createAgentClient,
  type AgentClient,
  type RuntimeSnapshotDto,
} from "@lyonbrown/agent-client";
import {
  bindDiscoveredEpoch,
  openSecureRunDirectory,
  type NativeControlPlaneProvider,
} from "@lyonbrown/local-control-plane";

import { verifyInstalledModelCache } from "@lyonbrown/workspace/internal/smoke-real-gguf-cache";
import type {
  RealGgufSmokePort,
  SmokeSnapshot,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-flow";
import {
  safeSnapshot,
  SnapshotReporter,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-report";
import {
  bounded,
  cleanupTemporaryAfterClose,
  runAllAttemptCleanup,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-lib";
import {
  appendBounded,
  assertDescriptor,
  processExit,
  removeOwnedRunDirectory,
  stopProcess,
  waitForDiscovery,
} from "@lyonbrown/workspace/internal/real-tui-blackbox-process";

export interface IndependentGgufSmokePortOptions {
  readonly agentdEntry: string;
  readonly environment: NodeJS.ProcessEnv;
  readonly fingerprint: string;
  readonly homeDirectory: string;
  readonly modelsDirectory: string;
  readonly nativeProvider: NativeControlPlaneProvider;
  readonly runDirectory: string;
  readonly scopeId: string;
  readonly scratchRoot: string;
  readonly cleanupRoot: string;
  readonly workspace: string;
  readonly startupTimeoutMs: number;
  readonly closeTimeoutMs: number;
}

export class IndependentGgufSmokePort implements RealGgufSmokePort {
  private daemon: ChildProcess | undefined;
  private client: AgentClient | undefined;
  private lastSnapshot: RuntimeSnapshotDto | undefined;
  private cleanupRemoved = false;
  private daemonOutput = "";
  private readonly reporter = new SnapshotReporter();

  public constructor(private readonly options: IndependentGgufSmokePortOptions) {}

  public async start(): Promise<void> {
    this.reporter.stage("daemon-start");
    const daemon = spawn(
      process.execPath,
      [
        this.options.agentdEntry,
        "--workspace",
        this.options.workspace,
        "--effective-config-fingerprint",
        this.options.fingerprint,
        "--scope-id",
        this.options.scopeId,
      ],
      {
        cwd: this.options.workspace,
        env: this.options.environment,
        shell: false,
        windowsHide: true,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    this.daemon = daemon;
    daemon.on("error", (error: Error) => {
      this.daemonOutput = appendBounded(this.daemonOutput, `child-process-error:${error.message}`);
    });
    daemon.stdout.on("data", (chunk: Buffer) => {
      this.daemonOutput = appendBounded(this.daemonOutput, chunk.toString("utf8"));
    });
    daemon.stderr.on("data", (chunk: Buffer) => {
      this.daemonOutput = appendBounded(this.daemonOutput, chunk.toString("utf8"));
    });
    const exited = processExit(daemon);
    const descriptor = await waitForDiscovery(
      () => openSecureRunDirectory(this.options.runDirectory, this.options.nativeProvider),
      daemon,
      exited,
      this.options.startupTimeoutMs,
      (candidate) => candidate.pid === daemon.pid && candidate.scopeId === this.options.scopeId,
    );
    assertDescriptor(descriptor, daemon, this.options.scopeId);
    const client = createAgentClient({
      binding: bindDiscoveredEpoch(descriptor),
      deadlines: {
        unaryMs: this.options.startupTimeoutMs,
        commandAttempts: 1,
        retryBackoffMs: 1,
      },
    });
    this.client = client;
    await client.health();
    const handshake = await client.handshake({
      clientName: "real-gguf-blackbox",
      clientVersion: "1",
    });
    if (
      handshake.serverInstanceId !== descriptor.serverInstanceId ||
      handshake.scopeId !== this.options.scopeId
    ) {
      throw new Error("REAL_GGUF_DAEMON_EPOCH_MISMATCH");
    }
    this.lastSnapshot = await client.getSnapshot();
    this.reporter.stage("grpc-ready", {
      daemonPid: descriptor.pid,
      endpoint: descriptor.endpoint,
      serverInstanceId: descriptor.serverInstanceId,
    });
  }

  public async readSnapshot(): Promise<RuntimeSnapshotDto> {
    const snapshot = await this.requireClient().getSnapshot();
    this.lastSnapshot = snapshot;
    this.reporter.snapshot(snapshot);
    return snapshot;
  }

  public startDownload(modelId: string): Promise<void> {
    this.reporter.stage("download-start");
    return this.enqueue({ type: "start-model-download", modelId });
  }

  public cancelDownload(operationId: string): Promise<void> {
    this.reporter.stage("download-cancel");
    return this.enqueue({ type: "cancel-model-download", operationId });
  }

  public startTurn(text: string): Promise<void> {
    this.reporter.stage("turn-start");
    return this.enqueue({ type: "start-turn", text });
  }

  public cancelTurn(): Promise<void> {
    this.reporter.stage("turn-cancel");
    return this.enqueue({ type: "cancel-turn" });
  }

  public respondApproval(requestId: string): Promise<void> {
    this.reporter.stage("approval-allow-once", { requestId });
    return this.enqueue({ type: "respond-approval", requestId, decision: "allow-once" });
  }

  public selectModel(selection: SmokeSnapshot["modelSelection"]): Promise<void> {
    this.reporter.stage("model-select", { kind: selection.kind });
    return this.enqueue({ type: "select-session-model", selection });
  }

  public async verifyWorkspaceFile(path: string, expectedContent: string): Promise<void> {
    const content = await readFile(join(this.options.workspace, path), "utf8");
    if (content !== expectedContent) throw new Error("REAL_GGUF_WORKSPACE_TOOL_OUTPUT_INVALID");
  }

  public async closeServer(): Promise<void> {
    const client = this.client;
    const daemon = this.daemon;
    await runAllAttemptCleanup([
      ...(client === undefined
        ? []
        : [
            async () => {
              await client.close();
              if (this.client === client) this.client = undefined;
            },
          ]),
      ...(daemon === undefined
        ? []
        : [
            async () => {
              await stopProcess(daemon, this.options.closeTimeoutMs);
              if (this.daemon === daemon) this.daemon = undefined;
            },
          ]),
    ]);
  }

  public restartServer(): Promise<void> {
    return this.start();
  }

  public verifyInstalledCache(expectedSha256: string): Promise<void> {
    return verifyInstalledModelCache(this.options.modelsDirectory, expectedSha256);
  }

  public cleanup(): Promise<void> {
    return cleanupTemporaryAfterClose(
      () => bounded("final-server-close", this.options.closeTimeoutMs, this.closeServer()),
      () => bounded("temporary-directory-cleanup", this.options.closeTimeoutMs, this.removeRoot()),
    );
  }

  public safeLastSnapshot(): unknown {
    return safeSnapshot(this.lastSnapshot);
  }

  public safeDaemonOutput(): string {
    return this.daemonOutput.slice(-4_096);
  }

  private requireClient(): AgentClient {
    if (this.client === undefined) throw new Error("REAL_GGUF_CLIENT_UNAVAILABLE");
    return this.client;
  }

  private async enqueue(command: Parameters<AgentClient["enqueue"]>[0]): Promise<void> {
    await this.requireClient().enqueue(command);
  }

  private async removeRoot(): Promise<void> {
    if (this.cleanupRemoved) return;
    await removeOwnedRunDirectory(this.options.scratchRoot, this.options.cleanupRoot);
    this.cleanupRemoved = true;
  }
}
