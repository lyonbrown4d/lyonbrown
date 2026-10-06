import { randomUUID } from "node:crypto";
import { Metadata } from "@grpc/grpc-js";
import type { EpochBinding } from "@lyonbrown/local-control-plane";
import {
  GetSnapshotRequest,
  HandshakeRequest,
  HealthRequest,
  HealthStatus,
  PROTOCOL_VERSION,
  RequestMetadata,
  SubscribeEventsRequest,
  bearerAuthorization,
  type HandshakeResponse,
  type HealthResponse,
  type CommandResponse,
  type RuntimeSnapshot,
} from "@lyonbrown/protocol";

import { mapCommand, type WireCommand } from "@lyonbrown/agent-client/internal/command";
import { clientError } from "@lyonbrown/agent-client/internal/errors";
import { mapHandshake } from "@lyonbrown/agent-client/internal/handshake";
import { mapSnapshot } from "@lyonbrown/agent-client/internal/snapshot";
import { isRetryableTransportError } from "@lyonbrown/agent-client/internal/status-error";
import {
  TransportFailure,
  createTransport,
  delay,
  eventIterable,
  normalizeDeadlines,
  publicError,
  staleEpoch,
  type NormalizedDeadlines,
  type TransportState,
} from "@lyonbrown/agent-client/internal/transport";
import {
  publicUnaryCall,
  unaryCall,
  type CapturedTransport,
} from "@lyonbrown/agent-client/internal/unary";
import type {
  AgentClient,
  AgentCommand,
  CommandReceiptDto,
  CreateAgentClientOptions,
  HandshakeDto,
  HandshakeInput,
  HealthDto,
  RuntimeEventDto,
  RuntimeSnapshotDto,
  SubscribeEventsInput,
} from "@lyonbrown/agent-client/internal/types";
import { requireId, requireNonNegativeRevision } from "@lyonbrown/agent-client/internal/validation";

export class DefaultAgentClient implements AgentClient {
  readonly #clientInstanceId: string;
  readonly #deadlines: NormalizedDeadlines;
  readonly #expectedScopeId: string;
  #state: TransportState;
  #bindingGeneration = 0;
  #handshakenEpoch: string | null = null;
  #closed = false;

  public constructor(options: CreateAgentClientOptions) {
    this.#clientInstanceId = requireId(
      "clientInstanceId",
      options.clientInstanceId ?? randomUUID(),
    );
    this.#deadlines = normalizeDeadlines(options.deadlines);
    this.#expectedScopeId = options.binding.scopeId;
    this.#state = createTransport(options.binding);
  }

  public async health(): Promise<HealthDto> {
    const captured = this.#capture();
    const response = await publicUnaryCall<HealthResponse>(
      this.#authorization(captured.state.binding),
      this.#deadlines.unaryMs,
      () => {
        this.#fence(captured);
      },
      (metadata, deadline, callback) => {
        captured.state.wire.health(
          HealthRequest.fromPartial({ metadata: this.#requestMetadata(randomUUID(), "") }),
          metadata,
          { deadline },
          callback,
        );
      },
    );
    if (
      response.protocolVersion !== PROTOCOL_VERSION ||
      response.status !== HealthStatus.HEALTH_STATUS_SERVING
    ) {
      throw clientError("protocol", "INVALID_RESPONSE", "health response is invalid");
    }
    return { protocolVersion: response.protocolVersion, status: "serving" };
  }

  public async handshake(input: HandshakeInput): Promise<HandshakeDto> {
    const captured = this.#capture();
    const clientName = requireId("clientName", input.clientName);
    const clientVersion = requireId("clientVersion", input.clientVersion);
    const response = await publicUnaryCall<HandshakeResponse>(
      this.#authorization(captured.state.binding),
      this.#deadlines.unaryMs,
      () => {
        this.#fence(captured);
      },
      (metadata, deadline, callback) => {
        captured.state.wire.handshake(
          HandshakeRequest.fromPartial({
            metadata: this.#requestMetadata(randomUUID(), ""),
            clientName,
            clientVersion,
          }),
          metadata,
          { deadline },
          callback,
        );
      },
    );
    const handshake = mapHandshake(response, captured.state.binding);
    this.#fence(captured);
    this.#handshakenEpoch = captured.state.binding.serverInstanceId;
    return handshake;
  }

  public enqueue(command: AgentCommand): Promise<CommandReceiptDto> {
    try {
      const state = this.#current();
      this.#requireHandshake(state.binding.serverInstanceId);
      const expectedGeneration = this.#bindingGeneration;
      const commandId = randomUUID();
      const expectedServerInstanceId = state.binding.serverInstanceId;
      const metadata = this.#requestMetadata(commandId, expectedServerInstanceId);
      const wireCommand = mapCommand(command, metadata);
      return this.#executeCommand(
        wireCommand,
        commandId,
        expectedServerInstanceId,
        expectedGeneration,
      );
    } catch (error: unknown) {
      return Promise.reject(publicError(error));
    }
  }

  public async getSnapshot(): Promise<RuntimeSnapshotDto> {
    const captured = this.#capture();
    this.#requireHandshake(captured.state.binding.serverInstanceId);
    const response = await publicUnaryCall<RuntimeSnapshot>(
      this.#authorization(captured.state.binding),
      this.#deadlines.unaryMs,
      () => {
        this.#fence(captured);
      },
      (metadata, deadline, callback) => {
        captured.state.wire.getSnapshot(
          GetSnapshotRequest.fromPartial({
            metadata: this.#requestMetadata(randomUUID(), captured.state.binding.serverInstanceId),
          }),
          metadata,
          { deadline },
          callback,
        );
      },
    );
    this.#fence(captured);
    return mapSnapshot(response);
  }

  public subscribeEvents(input: SubscribeEventsInput): AsyncIterable<RuntimeEventDto> {
    const captured = this.#capture();
    this.#requireHandshake(captured.state.binding.serverInstanceId);
    const afterRevision = requireNonNegativeRevision("afterRevision", input.afterRevision);
    const stream = captured.state.wire.subscribeEvents(
      SubscribeEventsRequest.fromPartial({
        metadata: this.#requestMetadata(randomUUID(), captured.state.binding.serverInstanceId),
        afterRevision,
      }),
      this.#authorization(captured.state.binding),
      {},
    );
    return eventIterable(stream, input.signal, afterRevision, () => {
      this.#fence(captured);
    });
  }

  public replaceBinding(binding: EpochBinding): void {
    this.#assertOpen();
    if (binding.scopeId !== this.#expectedScopeId) {
      throw clientError("protocol", "SCOPE_MISMATCH", "replacement binding scope does not match");
    }
    const replacement = createTransport(binding);
    const previous = this.#state;
    this.#bindingGeneration += 1;
    this.#state = replacement;
    this.#handshakenEpoch = null;
    previous.wire.close();
  }

  public close(): Promise<void> {
    if (this.#closed) return Promise.resolve();
    this.#closed = true;
    this.#handshakenEpoch = null;
    this.#state.wire.close();
    return Promise.resolve();
  }

  async #executeCommand(
    command: WireCommand,
    commandId: string,
    expectedServerInstanceId: string,
    expectedGeneration: number,
  ): Promise<CommandReceiptDto> {
    for (let attempt = 0; attempt < this.#deadlines.commandAttempts; attempt += 1) {
      const state = this.#current();
      this.#fenceExpected(expectedGeneration, expectedServerInstanceId);
      try {
        const response = await unaryCall<CommandResponse>(
          this.#authorization(state.binding),
          this.#deadlines.unaryMs,
          (metadata, callDeadline, callback) => {
            command.invoke(state.wire, metadata, callDeadline, callback);
          },
        );
        this.#fenceExpected(expectedGeneration, expectedServerInstanceId);
        if (
          response.protocolVersion !== PROTOCOL_VERSION ||
          response.commandId !== commandId ||
          !response.accepted
        ) {
          throw clientError("protocol", "INVALID_RECEIPT", "command receipt is invalid");
        }
        return { commandId, accepted: true };
      } catch (error: unknown) {
        this.#fenceExpected(expectedGeneration, expectedServerInstanceId);
        if (
          !(error instanceof TransportFailure) ||
          !isRetryableTransportError(error.serviceError) ||
          attempt + 1 >= this.#deadlines.commandAttempts
        ) {
          throw publicError(error);
        }
        await delay(this.#deadlines.retryBackoffMs);
      }
    }
    throw clientError("unavailable", "DISCONNECTED", "agent command failed");
  }

  #authorization(binding: EpochBinding): Metadata {
    const metadata = new Metadata();
    metadata.set("authorization", bearerAuthorization(binding.token.exposeForTransport()));
    return metadata;
  }

  #requestMetadata(commandId: string, expectedServerInstanceId: string): RequestMetadata {
    return RequestMetadata.fromPartial({
      protocolVersion: PROTOCOL_VERSION,
      clientInstanceId: this.#clientInstanceId,
      commandId,
      expectedServerInstanceId,
    });
  }

  #requireHandshake(epoch: string): void {
    if (this.#handshakenEpoch !== epoch) {
      throw clientError(
        "protocol",
        "NOT_HANDSHAKEN",
        "handshake is required for this server epoch",
      );
    }
  }

  #current(): TransportState {
    this.#assertOpen();
    return this.#state;
  }

  #capture(): CapturedTransport {
    return { state: this.#current(), generation: this.#bindingGeneration };
  }

  #fence(captured: CapturedTransport): void {
    this.#fenceExpected(captured.generation, captured.state.binding.serverInstanceId);
  }

  #fenceExpected(expectedGeneration: number, expectedEpoch: string): void {
    const actualEpoch = this.#state.binding.serverInstanceId;
    if (this.#bindingGeneration !== expectedGeneration || actualEpoch !== expectedEpoch) {
      throw staleEpoch(expectedEpoch, actualEpoch);
    }
  }

  #assertOpen(): void {
    if (this.#closed) throw clientError("cancelled", "CANCELLED", "agent client is closed");
  }
}
