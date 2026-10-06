import { AgentRuntimeError, type AgentRuntime } from "@lyonbrown/agent-runtime";
import { PROTOCOL_VERSION, RuntimeEvent, type RuntimeSnapshot } from "@lyonbrown/protocol";

import type { RuntimeCommand, RuntimeFailure, RuntimeHost, RuntimeResult } from "./runtime-host.ts";
import { assertProtocolRevision, projectRuntimeSnapshot } from "./runtime-adapter-projection.ts";

const NO_CAPABILITIES = Object.freeze([]) as readonly string[];
const OK = Object.freeze({ ok: true }) as RuntimeResult;
type SimpleRuntimeFailure = Extract<
  RuntimeFailure,
  {
    readonly code:
      | "RUNTIME_STOPPED"
      | "TURN_ALREADY_ACTIVE"
      | "NO_ACTIVE_TURN"
      | "EMPTY_TEXT"
      | "MODEL_SETUP_REQUIRED"
      | "MODEL_DOWNLOAD_UNAVAILABLE"
      | "NO_ACTIVE_MODEL_DOWNLOAD"
      | "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE";
  }
>;

class AgentRuntimeHostAdapter implements RuntimeHost {
  constructor(private readonly runtime: AgentRuntime) {}

  async execute(command: RuntimeCommand): Promise<RuntimeResult> {
    if (command.kind === "start-model-download" || command.kind === "cancel-model-download") {
      return failure("MODEL_DOWNLOAD_UNAVAILABLE");
    }
    if (command.kind === "start-turn") {
      if (empty(command.text)) return failure("EMPTY_TEXT");
      try {
        await this.runtime.startTurn(command.text);
        return OK;
      } catch (error: unknown) {
        return mapRuntimeFailure(error, command.kind);
      }
    }
    if (command.kind === "queue-follow-up") {
      if (empty(command.text)) return failure("EMPTY_TEXT");
      try {
        await this.runtime.followUp(command.text);
        return OK;
      } catch (error: unknown) {
        return mapRuntimeFailure(error, command.kind);
      }
    }
    if (command.kind !== "cancel-turn" && empty(command.text)) return failure("EMPTY_TEXT");
    try {
      switch (command.kind) {
        case "steer-current-turn":
          await this.runtime.steer(command.text);
          break;
        case "cancel-turn":
          this.runtime.cancel();
          break;
      }
      return OK;
    } catch (error: unknown) {
      return mapRuntimeFailure(error, command.kind);
    }
  }

  snapshot(): RuntimeSnapshot {
    return projectRuntimeSnapshot(this.runtime.snapshot());
  }

  async *subscribe(afterRevision: bigint, signal: AbortSignal): AsyncIterable<RuntimeEvent> {
    let lastRevision = assertProtocolRevision(afterRevision);
    const feed = this.runtime.snapshots();
    const close = (): void => {
      feed.close();
    };
    if (signal.aborted) {
      close();
      return;
    }
    signal.addEventListener("abort", close, { once: true });
    try {
      for (;;) {
        const next = await feed.next();
        if (next.done || isAborted(signal)) return;
        const snapshot = projectRuntimeSnapshot(next.value);
        if (snapshot.revision <= lastRevision) continue;
        yield RuntimeEvent.create({
          protocolVersion: PROTOCOL_VERSION,
          revision: snapshot.revision,
          snapshot,
        });
        lastRevision = snapshot.revision;
      }
    } finally {
      signal.removeEventListener("abort", close);
      await feed.return();
    }
  }

  capabilities(): readonly string[] {
    return NO_CAPABILITIES;
  }
}

export function createRuntimeHostAdapter(runtime: AgentRuntime): RuntimeHost {
  return new AgentRuntimeHostAdapter(runtime);
}

function isAborted(signal: AbortSignal): boolean {
  return signal.aborted;
}

function empty(value: string): boolean {
  return typeof value !== "string" || value.trim().length === 0;
}

function mapRuntimeFailure(error: unknown, command: RuntimeCommand["kind"]): RuntimeResult {
  if (!(error instanceof AgentRuntimeError)) return failure("RUNTIME_STOPPED");
  switch (error.code) {
    case "NO_ACTIVE_TURN":
      return failure("NO_ACTIVE_TURN");
    case "SESSION_BUSY":
      return failure("TURN_ALREADY_ACTIVE");
    case "KERNEL_FAILURE":
      return command === "steer-current-turn"
        ? { ok: false, error: { code: "STEERING", reason: "runtime rejected steering" } }
        : failure("RUNTIME_STOPPED");
    case "EVENT_SINK_UNAVAILABLE":
    case "EVENT_STREAM_CORRUPT":
    case "RUNTIME_DISPOSED":
    case "TERMINAL_SESSION":
      return failure("RUNTIME_STOPPED");
  }
}

function failure(code: SimpleRuntimeFailure["code"]): RuntimeResult {
  return { ok: false, error: { code } };
}
