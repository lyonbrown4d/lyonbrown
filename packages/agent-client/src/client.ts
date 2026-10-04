import { randomUUID } from "node:crypto";
import { Metadata, type ServiceError } from "@grpc/grpc-js";
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

import { mapCommand, type WireCommand } from "./command.js";
import { clientError } from "./errors.js";
import { mapSnapshot } from "./snapshot.js";
import { isRetryableTransportError } from "./status-error.js";
import {
  TransportFailure,
  createTransport,
  deadline,
  delay,
  eventIterable,
  normalizeDeadlines,
  publicError,
  staleEpoch,
  type NormalizedDeadlines,
  type TransportState,
} from "./transport.js";
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
} from "./types.js";
import { protocolId, requireId, requireNonNegativeRevision } from "./validation.js";

export class DefaultAgentClient implements AgentClient {
  readonly #clientInstanceId: string;
  readonly #deadlines: NormalizedDeadlines;
  #state: TransportState;
  #handshakenEpoch: string | null = null;
  #closed = false;

  public constructor(options: CreateAgentClientOptions) {
    this.#clientInstanceId = requireId(
      "clientInstanceId",
      options.clientInstanceId ?? randomUUID(),
    );
    this.#deadlines = normalizeDeadlines(options.deadlines);
    this.#state = createTransport(options.binding);
  }

  public async health(): Promise<HealthDto> {
    const state = this.#current();
    const response = await this.#unary<HealthResponse>(state, (metadata, deadline, callback) => {
      state.wire.health(
        HealthRequest.fromPartial({ metadata: this.#requestMetadata(randomUUID(), "") }),
        metadata,
        { deadline },
        callback,
      );
    });
    if (
      response.protocolVersion !== PROTOCOL_VERSION ||
      response.status !== HealthStatus.HEALTH_STATUS_SERVING
    ) {
      throw clientError("protocol", "INVALID_RESPONSE", "health response is invalid");
    }
    return { protocolVersion: response.protocolVersion, status: "serving" };
  }

  public async handshake(input: HandshakeInput): Promise<HandshakeDto> {
    const state = this.#current();
    const clientName = requireId("clientName", input.clientName);
    const clientVersion = requireId("clientVersion", input.clientVersion);
    const response = await this.#unary<HandshakeResponse>(state, (metadata, deadline, callback) => {
      state.wire.handshake(
        HandshakeRequest.fromPartial({
          metadata: this.#requestMetadata(randomUUID(), ""),
          clientName,
          clientVersion,
        }),
        metadata,
        { deadline },
        callback,
      );
    });
    this.#validateHandshake(response, state.binding);
    if (this.#state.binding.serverInstanceId !== state.binding.serverInstanceId) {
      throw staleEpoch(state.binding.serverInstanceId, this.#state.binding.serverInstanceId);
    }
    this.#handshakenEpoch = state.binding.serverInstanceId;
    return {
      protocolVersion: response.protocolVersion,
      serverVersion: protocolId("handshake.server_version", response.serverVersion),
      serverInstanceId: response.serverInstanceId,
      capabilities: response.capabilities.map((capability) =>
        protocolId("handshake.capability", capability),
      ),
      scopeId: response.scopeFingerprint,
    };
  }

  public enqueue(command: AgentCommand): Promise<CommandReceiptDto> {
    try {
      const state = this.#current();
      this.#requireHandshake(state.binding.serverInstanceId);
      const commandId = randomUUID();
      const expectedServerInstanceId = state.binding.serverInstanceId;
      const metadata = this.#requestMetadata(commandId, expectedServerInstanceId);
      const wireCommand = mapCommand(command, metadata);
      return this.#executeCommand(wireCommand, commandId, expectedServerInstanceId);
    } catch (error: unknown) {
      return Promise.reject(publicError(error));
    }
  }

  public async getSnapshot(): Promise<RuntimeSnapshotDto> {
    const state = this.#current();
    this.#requireHandshake(state.binding.serverInstanceId);
    const response = await this.#unary<RuntimeSnapshot>(state, (metadata, deadline, callback) => {
      state.wire.getSnapshot(
        GetSnapshotRequest.fromPartial({
          metadata: this.#requestMetadata(randomUUID(), state.binding.serverInstanceId),
        }),
        metadata,
        { deadline },
        callback,
      );
    });
    return mapSnapshot(response);
  }

  public subscribeEvents(input: SubscribeEventsInput): AsyncIterable<RuntimeEventDto> {
    const state = this.#current();
    this.#requireHandshake(state.binding.serverInstanceId);
    const afterRevision = requireNonNegativeRevision("afterRevision", input.afterRevision);
    const stream = state.wire.subscribeEvents(
      SubscribeEventsRequest.fromPartial({
        metadata: this.#requestMetadata(randomUUID(), state.binding.serverInstanceId),
        afterRevision,
      }),
      this.#authorization(state.binding),
      {},
    );
    return eventIterable(stream, input.signal, afterRevision);
  }

  public replaceBinding(binding: EpochBinding): void {
    this.#assertOpen();
    const previous = this.#state;
    this.#state = createTransport(binding);
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
  ): Promise<CommandReceiptDto> {
    for (let attempt = 0; attempt < this.#deadlines.commandAttempts; attempt += 1) {
      const state = this.#current();
      if (state.binding.serverInstanceId !== expectedServerInstanceId) {
        throw staleEpoch(expectedServerInstanceId, state.binding.serverInstanceId);
      }
      try {
        const response = await this.#unary<CommandResponse>(
          state,
          (metadata, callDeadline, callback) => {
            command.invoke(state.wire, metadata, callDeadline, callback);
          },
        );
        if (
          response.protocolVersion !== PROTOCOL_VERSION ||
          response.commandId !== commandId ||
          !response.accepted
        ) {
          throw clientError("protocol", "INVALID_RECEIPT", "command receipt is invalid");
        }
        return { commandId, accepted: true };
      } catch (error: unknown) {
        const currentEpoch = this.#state.binding.serverInstanceId;
        if (currentEpoch !== expectedServerInstanceId) {
          throw staleEpoch(expectedServerInstanceId, currentEpoch);
        }
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

  #unary<T>(
    state: TransportState,
    invoke: (
      metadata: Metadata,
      deadline: Date,
      callback: (error: ServiceError | null, response: T) => void,
    ) => void,
  ): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      invoke(
        this.#authorization(state.binding),
        deadline(this.#deadlines.unaryMs),
        (error, response) => {
          if (error !== null) reject(new TransportFailure(error));
          else resolve(response);
        },
      );
    });
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

  #validateHandshake(response: HandshakeResponse, binding: EpochBinding): void {
    if (response.protocolVersion !== PROTOCOL_VERSION) {
      throw clientError(
        "protocol",
        "INCOMPATIBLE_VERSION",
        "server protocol version is incompatible",
      );
    }
    if (response.serverInstanceId !== binding.serverInstanceId) {
      throw clientError(
        "protocol",
        "SERVER_INSTANCE_MISMATCH",
        "server instance does not match discovery",
        {
          expectedServerInstanceId: binding.serverInstanceId,
          actualServerInstanceId: response.serverInstanceId,
        },
      );
    }
    if (response.scopeFingerprint !== binding.scopeId) {
      throw clientError("protocol", "SCOPE_MISMATCH", "server scope does not match discovery");
    }
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

  #assertOpen(): void {
    if (this.#closed) throw clientError("cancelled", "CANCELLED", "agent client is closed");
  }
}
