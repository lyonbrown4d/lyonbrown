import { isDeepStrictEqual } from "node:util";

import {
  assertIdentifier,
  incrementRevision,
  parseAgentRuntimeEvent,
  reduceAgentRuntimeEvent,
  type AgentRuntimeEvent,
  type AgentRuntimeState,
  type SessionId,
} from "@lyonbrown/agent-domain";

import { runtimeError, type AgentRuntimeError } from "./errors.js";
import { applyStoredEvent } from "./recovery.js";
import type { RuntimeEventStorePort } from "./event-store-port.js";

export interface RuntimeEventWriterOptions {
  readonly sessionId: SessionId;
  readonly eventStore: RuntimeEventStorePort;
  readonly initialState: AgentRuntimeState;
  readonly createEventId: () => string;
  readonly now: () => string;
  readonly publish: (state: AgentRuntimeState) => void;
}

export class RuntimeEventWriter {
  #state: AgentRuntimeState;
  #tail: Promise<void> = Promise.resolve();
  #fatal: AgentRuntimeError | undefined;

  constructor(private readonly options: RuntimeEventWriterOptions) {
    this.#state = options.initialState;
  }

  get state(): AgentRuntimeState {
    return this.#state;
  }

  get fatal(): AgentRuntimeError | undefined {
    return this.#fatal;
  }

  append(input: AgentRuntimeEvent): Promise<void> {
    let event: AgentRuntimeEvent;
    try {
      event = parseAgentRuntimeEvent(input);
    } catch (error: unknown) {
      return Promise.reject(
        error instanceof Error
          ? error
          : runtimeError("EVENT_STREAM_CORRUPT", "Durable runtime event stream is invalid"),
      );
    }
    const operation = this.#tail.then(() => this.#appendOne(event));
    this.#tail = operation.catch(() => undefined);
    return operation;
  }

  async #appendOne(event: AgentRuntimeEvent): Promise<void> {
    if (this.#fatal !== undefined) throw this.#fatal;
    const candidate = reduceAgentRuntimeEvent(this.#state, {
      revision: incrementRevision(this.#state.revision),
      event,
    });
    let eventId: string;
    let stored;
    try {
      eventId = assertIdentifier(this.options.createEventId(), "eventId");
      stored = await this.options.eventStore.append(this.options.sessionId, this.#state.revision, [
        {
          eventId,
          type: event.type,
          occurredAt: this.options.now(),
          payload: event,
        },
      ]);
    } catch {
      throw this.#markFatal();
    }
    if (stored.length !== 1 || stored[0]?.eventId !== eventId) throw this.#markFatal();
    let recoveredCandidate: AgentRuntimeState;
    try {
      recoveredCandidate = applyStoredEvent(this.#state, stored[0], this.options.sessionId);
    } catch {
      throw this.#markFatal();
    }
    if (!isDeepStrictEqual(candidate, recoveredCandidate)) throw this.#markFatal();
    this.#state = candidate;
    this.options.publish(this.#state);
  }

  #markFatal(): AgentRuntimeError {
    this.#fatal ??= runtimeError(
      "EVENT_SINK_UNAVAILABLE",
      "Durable runtime event sink unavailable",
    );
    return this.#fatal;
  }
}
