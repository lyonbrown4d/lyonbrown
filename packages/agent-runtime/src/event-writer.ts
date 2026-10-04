import {
  assertIdentifier,
  parseAgentRuntimeEvent,
  type AgentRuntimeEvent,
  type AgentRuntimeState,
  type SessionId,
} from "@lyonbrown/agent-domain";

import { runtimeError, type AgentRuntimeError } from "./errors.js";
import { applyStoredEvent } from "./recovery.js";
import type { RuntimeEventStorePort } from "./types.js";

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
    let resolveResult: (() => void) | undefined;
    let rejectResult: ((error: AgentRuntimeError) => void) | undefined;
    const result = new Promise<void>((resolve, reject: (error: AgentRuntimeError) => void) => {
      resolveResult = resolve;
      rejectResult = reject;
    });
    this.#tail = this.#tail.then(async () => {
      try {
        await this.#appendOne(parseAgentRuntimeEvent(input));
        resolveResult?.();
      } catch {
        const fatal = this.#fatal ?? this.#markFatal();
        rejectResult?.(fatal);
      }
    });
    return result;
  }

  async #appendOne(event: AgentRuntimeEvent): Promise<void> {
    if (this.#fatal !== undefined) throw this.#fatal;
    const eventId = assertIdentifier(this.options.createEventId(), "eventId");
    let stored;
    try {
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
    try {
      this.#state = applyStoredEvent(this.#state, stored[0], this.options.sessionId);
    } catch {
      throw this.#markFatal();
    }
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
