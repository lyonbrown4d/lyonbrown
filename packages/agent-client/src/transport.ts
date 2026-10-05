import { isIP } from "node:net";
import { ChannelCredentials, type ClientReadableStream, type ServiceError } from "@grpc/grpc-js";
import { SecretToken, type EpochBinding } from "@lyonbrown/local-control-plane";
import {
  AgentServiceClient,
  GRPC_CHANNEL_OPTIONS,
  bearerAuthorization,
  type RuntimeEvent,
} from "@lyonbrown/protocol";

import { AgentClientError, clientError } from "./errors.ts";
import { mapEvent } from "./snapshot.ts";
import { mapServiceError } from "./status-error.ts";
import type { CreateAgentClientOptions, RuntimeEventDto } from "./types.ts";

export interface NormalizedDeadlines {
  readonly unaryMs: number;
  readonly commandAttempts: number;
  readonly retryBackoffMs: number;
}

export interface TransportState {
  readonly binding: EpochBinding;
  readonly wire: AgentServiceClient;
}

export class TransportFailure extends Error {
  public constructor(public readonly serviceError: ServiceError) {
    super("grpc transport failure");
  }
}

export function createTransport(binding: EpochBinding): TransportState {
  validateBinding(binding);
  return {
    binding,
    wire: new AgentServiceClient(
      binding.endpoint,
      ChannelCredentials.createInsecure(),
      GRPC_CHANNEL_OPTIONS,
    ),
  };
}

function validateBinding(binding: EpochBinding): void {
  if (!isLoopbackEndpoint(binding.endpoint)) {
    throw clientError("validation", "INVALID_ARGUMENT", "binding endpoint must be loopback");
  }
  if (!(binding.token instanceof SecretToken)) {
    throw clientError("validation", "INVALID_ARGUMENT", "binding token capability is invalid");
  }
  bearerAuthorization(binding.token.exposeForTransport());
  if (!/^[a-f0-9]{64}$/u.test(binding.scopeId)) {
    throw clientError("validation", "INVALID_ARGUMENT", "binding scope is invalid");
  }
  if (
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/iu.test(
      binding.serverInstanceId,
    )
  ) {
    throw clientError("validation", "INVALID_ARGUMENT", "binding server instance is invalid");
  }
}

function isLoopbackEndpoint(endpoint: string): boolean {
  const ipv6 = /^\[([^\]]+)\]:(\d{1,5})$/u.exec(endpoint);
  if (ipv6?.[1] !== undefined && ipv6[2] !== undefined) {
    return ipv6[1] === "::1" && validPort(ipv6[2]);
  }
  const separator = endpoint.lastIndexOf(":");
  if (separator <= 0) return false;
  const host = endpoint.slice(0, separator);
  return (
    isIP(host) === 4 && host.split(".")[0] === "127" && validPort(endpoint.slice(separator + 1))
  );
}

function validPort(encoded: string): boolean {
  if (!/^\d{1,5}$/u.test(encoded)) return false;
  const port = Number(encoded);
  return port >= 1 && port <= 65_535;
}

export function normalizeDeadlines(
  input: CreateAgentClientOptions["deadlines"],
): NormalizedDeadlines {
  return {
    unaryMs: boundedInteger("deadlines.unaryMs", input?.unaryMs ?? 30_000, 300_000),
    commandAttempts: boundedInteger("deadlines.commandAttempts", input?.commandAttempts ?? 3, 10),
    retryBackoffMs: boundedInteger(
      "deadlines.retryBackoffMs",
      input?.retryBackoffMs ?? 100,
      60_000,
    ),
  };
}

function boundedInteger(field: string, value: number, maximum: number): number {
  if (!Number.isSafeInteger(value) || value <= 0 || value > maximum) {
    throw clientError(
      "validation",
      "INVALID_ARGUMENT",
      `${field} must be a positive integer no greater than ${String(maximum)}`,
    );
  }
  return value;
}

export function deadline(milliseconds: number): Date {
  return new Date(Date.now() + milliseconds);
}

export function staleEpoch(expected: string, actual: string): AgentClientError {
  return clientError("unavailable", "STALE_SERVER_EPOCH", "agent daemon epoch changed", {
    expectedServerInstanceId: expected,
    actualServerInstanceId: actual,
  });
}

export function publicError(error: unknown): AgentClientError {
  if (error instanceof AgentClientError) return error;
  if (error instanceof TransportFailure) return mapServiceError(error.serviceError);
  return clientError("protocol", "INVALID_RESPONSE", "agent request failed unexpectedly");
}

export function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

export function eventIterable(
  stream: ClientReadableStream<RuntimeEvent>,
  signal: AbortSignal | undefined,
  afterRevision: bigint,
  fence: () => void,
): AsyncIterable<RuntimeEventDto> {
  const terminal: { error?: Error } = {};
  const captureError = (error: Error): void => {
    terminal.error = error;
  };
  stream.on("error", captureError);
  const cancel = (): void => {
    stream.cancel();
  };
  if (signal?.aborted === true) {
    stream.cancel();
  } else {
    signal?.addEventListener("abort", cancel, { once: true });
  }
  return {
    [Symbol.asyncIterator]: () =>
      iterateEvents(stream, signal, cancel, afterRevision, fence, terminal, captureError),
  };
}

async function* iterateEvents(
  stream: ClientReadableStream<RuntimeEvent>,
  signal: AbortSignal | undefined,
  cancel: () => void,
  afterRevision: bigint,
  fence: () => void,
  terminal: { error?: Error },
  captureError: (error: Error) => void,
): AsyncGenerator<RuntimeEventDto> {
  let revision = afterRevision;
  try {
    if (signal?.aborted === true) {
      throw clientError("cancelled", "CANCELLED", "event subscription was cancelled");
    }
    if (terminal.error !== undefined) {
      throw terminal.error;
    }
    const events = stream as unknown as AsyncIterable<RuntimeEvent>;
    for await (const wire of events) {
      fence();
      const event = mapEvent(wire);
      if (event.revision <= revision) {
        throw clientError("protocol", "REVISION_MISMATCH", "event revisions are not increasing", {
          previousRevision: revision,
          eventRevision: event.revision,
        });
      }
      revision = event.revision;
      fence();
      yield event;
    }
  } catch (error: unknown) {
    fence();
    throw publicError(isServiceError(error) ? new TransportFailure(error) : error);
  } finally {
    signal?.removeEventListener("abort", cancel);
    stream.removeListener("error", captureError);
    stream.cancel();
  }
}

function isServiceError(value: unknown): value is ServiceError {
  if (typeof value !== "object" || value === null) return false;
  try {
    return typeof Reflect.get(value, "code") === "number";
  } catch {
    return false;
  }
}
