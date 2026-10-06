import type { AgentRuntimeState, SessionId } from "@lyonbrown/agent-domain";
import type { KernelEventSink, KernelSession } from "@lyonbrown/kernel-pi";

import type { RuntimeEventStorePort } from "@lyonbrown/agent-runtime/internal/event-store-port";

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
}

export interface RuntimeSnapshotFeed extends AsyncIterableIterator<AgentRuntimeState> {
  close(): void;
  return(): Promise<IteratorResult<AgentRuntimeState>>;
  throw(error?: unknown): Promise<IteratorResult<AgentRuntimeState>>;
}

export interface AgentRuntime {
  snapshot(): AgentRuntimeState;
  snapshots(): RuntimeSnapshotFeed;
  startTurn(input: string, options?: { readonly signal?: AbortSignal }): Promise<void>;
  steer(input: string): Promise<void>;
  followUp(input: string): Promise<void>;
  cancel(): void;
  dispose(): Promise<void>;
}
