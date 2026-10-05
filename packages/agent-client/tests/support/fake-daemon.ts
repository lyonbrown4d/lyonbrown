import { randomUUID } from "node:crypto";
import {
  Server,
  ServerCredentials,
  type Metadata,
  type ServerUnaryCall,
  type sendUnaryData,
} from "@grpc/grpc-js";
import {
  SecretToken,
  bindDiscoveredEpoch,
  createDiscoveryDescriptor,
  type EpochBinding,
} from "@lyonbrown/local-control-plane";
import {
  AgentServiceService,
  HealthStatus,
  ModelSelection,
  PROTOCOL_VERSION,
  RuntimePhase,
  RuntimeSnapshot,
  type AgentServiceServer,
  type CommandResponse,
  type HandshakeResponse,
  type RequestMetadata,
  type StartTurnRequest,
} from "@lyonbrown/protocol";

import { createAgentClient, type AgentClient } from "../../src/index.js";

const SCOPE = "a".repeat(64);
const poolId = Number.parseInt(process.env.VITEST_POOL_ID ?? "1", 10);
let nextTestPort = 32_000 + (Number.isSafeInteger(poolId) ? poolId : 1) * 200;

function allocateTestPort(): number {
  nextTestPort += 1;
  return nextTestPort;
}

export interface RecordedCall {
  readonly authorization: string;
  readonly commandId: string;
  readonly expectedServerInstanceId: string;
}

export interface FakeOptions {
  readonly serverInstanceId?: string;
  readonly scopeId?: string;
  readonly startTurn?: (
    call: ServerUnaryCall<StartTurnRequest, CommandResponse>,
    callback: sendUnaryData<CommandResponse>,
  ) => void;
  readonly subscribe?: AgentServiceServer["subscribeEvents"];
  readonly health?: AgentServiceServer["health"];
  readonly handshake?: AgentServiceServer["handshake"];
  readonly getSnapshot?: AgentServiceServer["getSnapshot"];
  readonly snapshot?: RuntimeSnapshot;
}

export interface FakeDaemon {
  readonly endpoint: string;
  readonly token: string;
  readonly serverInstanceId: string;
  readonly calls: RecordedCall[];
  readonly commands: string[];
  readonly authorizationHeaders: string[];
  close(): Promise<void>;
}

const daemons: FakeDaemon[] = [];

export async function closeDaemons(): Promise<void> {
  await Promise.all(daemons.splice(0).map(async (daemon) => daemon.close()));
}

function metadataValue(metadata: Metadata, key: string): string {
  const value = metadata.get(key)[0];
  return typeof value === "string" ? value : "";
}

function record(call: ServerUnaryCall<StartTurnRequest, CommandResponse>): RecordedCall {
  return {
    authorization: metadataValue(call.metadata, "authorization"),
    commandId: call.request.metadata?.commandId ?? "",
    expectedServerInstanceId: call.request.metadata?.expectedServerInstanceId ?? "",
  };
}

export async function spawnDaemon(options: FakeOptions = {}): Promise<FakeDaemon> {
  const server = new Server();
  const token = Buffer.alloc(32, daemons.length + 1).toString("base64url");
  const serverInstanceId = options.serverInstanceId ?? randomUUID();
  const calls: RecordedCall[] = [];
  const commands: string[] = [];
  const authorizationHeaders: string[] = [];
  const snapshot =
    options.snapshot ??
    RuntimeSnapshot.fromPartial({
      protocolVersion: PROTOCOL_VERSION,
      revision: 1n,
      phase: RuntimePhase.RUNTIME_PHASE_READY,
      modelSelection: ModelSelection.fromPartial({ selection: { $case: "auto", auto: true } }),
    });
  const handshake: HandshakeResponse = {
    $type: "lyonbrown.agent.v1.HandshakeResponse",
    protocolVersion: PROTOCOL_VERSION,
    serverVersion: "test",
    serverInstanceId,
    capabilities: ["events"],
    scopeFingerprint: options.scopeId ?? SCOPE,
  };
  const accept = (
    name: string,
    call: {
      readonly metadata: Metadata;
      readonly request: { readonly metadata: RequestMetadata | undefined };
    },
    callback: sendUnaryData<CommandResponse>,
  ): void => {
    commands.push(name);
    calls.push({
      authorization: metadataValue(call.metadata, "authorization"),
      commandId: call.request.metadata?.commandId ?? "",
      expectedServerInstanceId: call.request.metadata?.expectedServerInstanceId ?? "",
    });
    callback(null, {
      $type: "lyonbrown.agent.v1.CommandResponse",
      protocolVersion: PROTOCOL_VERSION,
      commandId: call.request.metadata?.commandId ?? "",
      accepted: true,
    });
  };
  const recordAuthorization = (call: { readonly metadata: Metadata }): void => {
    authorizationHeaders.push(metadataValue(call.metadata, "authorization"));
  };
  const implementation: AgentServiceServer = {
    health:
      options.health ??
      ((call, callback) => {
        recordAuthorization(call);
        callback(null, {
          $type: "lyonbrown.agent.v1.HealthResponse",
          protocolVersion: PROTOCOL_VERSION,
          status: HealthStatus.HEALTH_STATUS_SERVING,
        });
      }),
    handshake:
      options.handshake ??
      ((call, callback) => {
        recordAuthorization(call);
        callback(null, handshake);
      }),
    startTurn: (call, callback) => {
      if (options.startTurn !== undefined) {
        calls.push(record(call));
        options.startTurn(call, callback);
      } else accept("start-turn", call, callback);
    },
    steerCurrentTurn: (call, callback) => {
      accept("steer-current-turn", call, callback);
    },
    queueFollowUp: (call, callback) => {
      accept("queue-follow-up", call, callback);
    },
    cancelTurn: (call, callback) => {
      accept("cancel-turn", call, callback);
    },
    forceStop: (call, callback) => {
      accept("force-stop", call, callback);
    },
    respondApproval: (call, callback) => {
      accept("respond-approval", call, callback);
    },
    selectSessionModel: (call, callback) => {
      accept("select-session-model", call, callback);
    },
    startModelDownload: (call, callback) => {
      accept("start-model-download", call, callback);
    },
    cancelModelDownload: (call, callback) => {
      accept("cancel-model-download", call, callback);
    },
    getSnapshot:
      options.getSnapshot ??
      ((call, callback) => {
        recordAuthorization(call);
        callback(null, snapshot);
      }),
    subscribeEvents:
      options.subscribe ??
      ((call) => {
        recordAuthorization(call);
        call.end();
      }),
  };
  server.addService(AgentServiceService, implementation);
  const port = await new Promise<number>((resolve, reject) => {
    server.bindAsync(
      `127.0.0.1:${String(allocateTestPort())}`,
      ServerCredentials.createInsecure(),
      (error, boundPort) => {
        if (error !== null) reject(error);
        else resolve(boundPort);
      },
    );
  });
  const daemon: FakeDaemon = {
    endpoint: `127.0.0.1:${String(port)}`,
    token,
    serverInstanceId,
    calls,
    commands,
    authorizationHeaders,
    close: async () => {
      await new Promise<void>((resolve) => {
        server.tryShutdown(() => {
          resolve();
        });
      });
    },
  };
  daemons.push(daemon);
  return daemon;
}

export function bindingFor(daemon: FakeDaemon, scopeId = SCOPE): EpochBinding {
  const descriptor = createDiscoveryDescriptor({
    endpoint: daemon.endpoint,
    pid: process.pid,
    scopeId,
    serverInstanceId: daemon.serverInstanceId,
    token: SecretToken.parse(daemon.token),
  });
  return bindDiscoveredEpoch(descriptor);
}

export function clientFor(daemon: FakeDaemon): AgentClient {
  return createAgentClient({
    binding: bindingFor(daemon),
    deadlines: { unaryMs: 500, commandAttempts: 2, retryBackoffMs: 1 },
  });
}
