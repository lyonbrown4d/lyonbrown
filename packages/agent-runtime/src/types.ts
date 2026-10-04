import type { AgentRuntimeState, SessionId } from "@lyonbrown/agent-domain";
import type { EventStore } from "@lyonbrown/event-store";
import type { KernelEventSink, KernelSession } from "@lyonbrown/kernel-pi";

export type RuntimeEventStorePort = Pick<EventStore, "append" | "currentRevision" | "load">;

export interface KernelSessionFactoryInput {
  readonly sessionId: string;
  readonly eventSink: KernelEventSink;
}

export type KernelSessionFactory = (input: KernelSessionFactoryInput) => KernelSession;

export interface CreateAgentRuntimeOptions {
  readonly sessionId: SessionId;
  /** The caller owns this port and remains responsible for closing it. */
  readonly eventStore: RuntimeEventStorePort;
  /** Creates the adapter session; the runtime disposes only the returned session. */
  readonly createKernelSession: KernelSessionFactory;
  readonly createEventId?: () => string;
  readonly now?: () => string;
  readonly maxRecoveryEvents?: number;
}

export interface RuntimeSnapshotFeed extends AsyncIterableIterator<AgentRuntimeState> {
  close(): void;
}

export interface AgentRuntime {
  snapshot(): AgentRuntimeState;
  snapshots(): RuntimeSnapshotFeed;
  startTurn(input: string, options?: { readonly signal?: AbortSignal }): Promise<void>;
  steer(input: string): void;
  followUp(input: string): void;
  cancel(): void;
  dispose(): void;
}
