import { Metadata, type ClientUnaryCall, type ServiceError } from "@grpc/grpc-js";
import type { SecretToken } from "@lyonbrown/local-control-plane";
import {
  CommandErrorDetail,
  PROTOCOL_VERSION,
  RequestMetadata,
  RuntimeEvent,
  RuntimeSnapshot,
  bearerAuthorization,
  type AgentServiceClient,
  type CommandResponse,
  type ForceStopRequest,
  type HealthRequest,
  type HealthResponse,
  type StartTurnRequest,
} from "@lyonbrown/protocol";
import { vi } from "vitest";
import type { RuntimeCommand, RuntimeHost, RuntimeResult } from "../node-src/runtime-host.js";

export const SERVER_INSTANCE_ID = "018f47d6-7c12-7b19-a0b6-ff87748f42bc";
export const SCOPE_FINGERPRINT = "a".repeat(64);

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
