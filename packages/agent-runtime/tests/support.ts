import type { EventStore } from "@lyonbrown/event-store";
import type { KernelEvent, KernelEventSink, KernelSession } from "@lyonbrown/kernel-pi";

import {
  createRuntimeEventStorePort,
  type RuntimeEventStorePort,
  type RuntimeEventStoreSource,
} from "@lyonbrown/agent-runtime/internal/index";

export class CapturingKernelFactory {
  sink: KernelEventSink | undefined;
  readonly starts: string[] = [];
  readonly approvalResponses: { readonly requestId: string; readonly decision: string }[] = [];
  approvalResponseStatus: "accepted" | "no-pending" | "mismatch" = "accepted";
  startBehavior: ((input: string, sink: KernelEventSink) => Promise<void>) | undefined;

  create = ({ eventSink }: { readonly eventSink: KernelEventSink }): KernelSession => {
    this.sink = eventSink;
    return {
      startTurn: async (input) => {
        this.starts.push(input);
        await this.startBehavior?.(input, eventSink);
      },
      steer: () => undefined,
      followUp: () => undefined,
      cancel: () => undefined,
      respondApproval: (requestId, decision) => {
        this.approvalResponses.push({ requestId, decision });
        return this.approvalResponseStatus;
      },
      subscribe: () => () => undefined,
      dispose: () => undefined,
    };
  };

  async emit(event: KernelEvent): Promise<void> {
    if (this.sink === undefined) throw new Error("kernel sink not captured");
    await this.sink.append(event);
  }
}

export function sequentialEventIds(prefix = "event"): () => string {
  let next = 0;
  return () => `${prefix}-${String(++next)}`;
}

export function runtimeEventStore(store: RuntimeEventStoreSource): RuntimeEventStorePort {
  return createRuntimeEventStorePort(store);
}

export async function eventTypes(store: EventStore, sessionId: string): Promise<readonly string[]> {
  return (await store.load(sessionId, { afterRevision: 0n, limit: 1_000 })).map(
    (event) => event.type,
  );
}
