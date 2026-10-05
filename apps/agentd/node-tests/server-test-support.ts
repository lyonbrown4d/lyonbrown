import { Metadata, credentials, type ClientUnaryCall, type ServiceError } from "@grpc/grpc-js";
import { SecretToken } from "@lyonbrown/local-control-plane";
import {
  CommandErrorDetail,
  GRPC_CHANNEL_OPTIONS,
  PROTOCOL_VERSION,
  RequestMetadata,
  RuntimeEvent,
  RuntimeSnapshot,
  AgentServiceClient,
  bearerAuthorization,
  type CommandResponse,
  type ForceStopRequest,
  type HealthRequest,
  type HealthResponse,
  type StartTurnRequest,
} from "@lyonbrown/protocol";
import { vi } from "vitest";
import type { RuntimeCommand, RuntimeHost, RuntimeResult } from "../node-src/runtime-host.js";
import { startAgentdServer, type RunningAgentdServer } from "../node-src/server.js";

export const SERVER_INSTANCE_ID = "018f47d6-7c12-7b19-a0b6-ff87748f42bc";
export const SCOPE_FINGERPRINT = "a".repeat(64);

const poolId = Number.parseInt(process.env.VITEST_POOL_ID ?? "1", 10);
let nextTestPort = 30_000 + (Number.isSafeInteger(poolId) ? poolId : 1) * 200;

export function allocateTestPort(): number {
  nextTestPort += 1;
  return nextTestPort;
}

export function requestMetadata(commandId: string, epoch = SERVER_INSTANCE_ID): RequestMetadata {
  return RequestMetadata.create({
    protocolVersion: PROTOCOL_VERSION,
    clientInstanceId: "test-client",
    commandId,
    expectedServerInstanceId: epoch,
  });
}

export class FakeHost implements RuntimeHost {
  public readonly execute = vi.fn(
    async (_command: RuntimeCommand): Promise<RuntimeResult> => ({ ok: true }),
  );

  public snapshot(): RuntimeSnapshot {
    return RuntimeSnapshot.create({ protocolVersion: PROTOCOL_VERSION, revision: 1n });
  }

  public async *subscribe(
    _afterRevision: bigint,
    signal: AbortSignal,
  ): AsyncIterable<RuntimeEvent> {
    if (!signal.aborted) {
      yield RuntimeEvent.create({
        protocolVersion: PROTOCOL_VERSION,
        revision: 1n,
        snapshot: this.snapshot(),
      });
    }
  }

  public capabilities(): readonly string[] {
    return ["runtime.test.v1"];
  }
}

export interface TestServerState {
  readonly address: string;
  readonly client: AgentServiceClient;
  readonly host: FakeHost;
  readonly running: RunningAgentdServer;
  readonly token: SecretToken;
  close(): Promise<void>;
}

export async function startTestServer(host = new FakeHost()): Promise<TestServerState> {
  const token = SecretToken.generate();
  const running = await startAgentdServer({
    host,
    token,
    serverInstanceId: SERVER_INSTANCE_ID,
    scopeFingerprint: SCOPE_FINGERPRINT,
    port: allocateTestPort(),
  });
  const client = new AgentServiceClient(running.address, credentials.createInsecure(), {
    ...GRPC_CHANNEL_OPTIONS,
  });
  return {
    address: running.address,
    client,
    host,
    running,
    token,
    async close() {
      client.close();
      await running.close();
    },
  };
}

export function authorization(token: SecretToken): Metadata {
  const metadata = new Metadata();
  metadata.set("authorization", bearerAuthorization(token.exposeForTransport()));
  return metadata;
}

export function health(
  client: AgentServiceClient,
  request: HealthRequest,
  metadata = new Metadata(),
): Promise<HealthResponse> {
  return invokeUnary((callback) => client.health(request, metadata, callback));
}

export function startTurn(
  client: AgentServiceClient,
  request: StartTurnRequest,
  metadata: Metadata,
): Promise<CommandResponse> {
  return invokeUnary((callback) => client.startTurn(request, metadata, callback));
}

export function forceStop(
  client: AgentServiceClient,
  request: ForceStopRequest,
  metadata: Metadata,
): Promise<CommandResponse> {
  return invokeUnary((callback) => client.forceStop(request, metadata, callback));
}

export function invokeUnary<Response>(
  invoke: (callback: (error: ServiceError | null, response: Response) => void) => ClientUnaryCall,
): Promise<Response> {
  return new Promise((resolve, reject) => {
    invoke((error, response) => {
      if (error === null) resolve(response);
      else reject(error);
    });
  });
}

export function detailFrom(error: unknown): CommandErrorDetail {
  const serviceError = error as ServiceError;
  const raw = serviceError.metadata.get("grpc-status-details-bin")[0];
  if (!Buffer.isBuffer(raw)) throw new Error("missing raw command detail");
  return CommandErrorDetail.decode(raw);
}
