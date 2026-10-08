import type {
  AgentRuntimeState,
  RuntimeDecisionFallbackReason,
  RuntimeDecisionIntent,
  RuntimeDecisionRisk,
  RuntimeJsonValue,
  RuntimeModelSelection,
  SessionId,
} from "@lyonbrown/agent-domain";
import type { KernelEventSink, KernelSession } from "@lyonbrown/kernel-pi";

import type { RuntimeEventStorePort } from "@lyonbrown/agent-runtime/internal/event-store-port";

export interface KernelSessionFactoryInput {
  readonly sessionId: string;
  readonly eventSink: KernelEventSink;
  readonly state: AgentRuntimeState;
}

export type KernelSessionFactory = (input: KernelSessionFactoryInput) => KernelSession;

export type RuntimePreKernelDecision =
  | {
      readonly kind: "decided";
      readonly questionVersion: string;
      readonly providerId: string;
      readonly modelId: string;
      readonly modelRevision: string;
      readonly intent: RuntimeDecisionIntent;
      readonly risk: RuntimeDecisionRisk;
      readonly requiresTools: boolean;
      readonly confidence: number;
      readonly probabilities: RuntimeJsonValue;
      readonly action: "continue_pi";
    }
  | {
      readonly kind: "fallback";
      readonly questionVersion: string;
      readonly reason: RuntimeDecisionFallbackReason;
      readonly action: "continue_pi";
    };

export interface RuntimeDecisionPort {
  evaluate(request: {
    readonly input: string;
    readonly signal: AbortSignal;
  }): Promise<RuntimePreKernelDecision>;
}

export interface CreateAgentRuntimeOptions {
  readonly sessionId: SessionId;
  /** The caller owns this port and remains responsible for closing it. */
  readonly eventStore: RuntimeEventStorePort;
  /** Creates the adapter session; the runtime disposes only the returned session. */
  readonly createKernelSession: KernelSessionFactory;
  readonly decision?: RuntimeDecisionPort;
  readonly createEventId?: () => string;
  readonly now?: () => string;
}

export interface RuntimeSnapshotFeed extends AsyncIterableIterator<AgentRuntimeState> {
  close(): void;
  return(): Promise<IteratorResult<AgentRuntimeState>>;
  throw(error?: unknown): Promise<IteratorResult<AgentRuntimeState>>;
}

export type RuntimeApprovalDecision =
  | "allow-once"
  | "allow-for-session"
  | "deny-once"
  | "deny-remaining";

export interface AgentRuntime {
  snapshot(): AgentRuntimeState;
  snapshots(): RuntimeSnapshotFeed;
  startTurn(input: string, options?: { readonly signal?: AbortSignal }): Promise<void>;
  steer(input: string): Promise<void>;
  followUp(input: string): Promise<void>;
  cancel(): void;
  respondApproval(requestId: string, decision: RuntimeApprovalDecision): void;
  selectModel(selection: RuntimeModelSelection): Promise<void>;
  dispose(): Promise<void>;
}
