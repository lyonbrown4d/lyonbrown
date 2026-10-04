import {
  asSessionId,
  createInitialAgentRuntimeState,
  reduceAgentRuntimeEvent,
  type AgentRuntimeEvent,
  type AgentRuntimeState,
} from "@lyonbrown/agent-domain";
import type { AgentRuntime, RuntimeSnapshotFeed } from "@lyonbrown/agent-runtime";

type WithoutSession<T> = T extends { readonly sessionId: unknown } ? Omit<T, "sessionId"> : never;
type SessionlessAgentRuntimeEvent = WithoutSession<AgentRuntimeEvent>;

export function stateFrom(
  events: readonly SessionlessAgentRuntimeEvent[],
  session = "adapter-session",
): AgentRuntimeState {
  const sessionId = asSessionId(session);
  return events.reduce(
    (state, event, index) =>
      reduceAgentRuntimeEvent(state, {
        revision: BigInt(index + 1) as AgentRuntimeState["revision"],
        event: { ...event, sessionId } as AgentRuntimeEvent,
      }),
    createInitialAgentRuntimeState(sessionId),
  );
}

export const openedState = (): AgentRuntimeState => stateFrom([{ type: "session_opened" }]);

export const activeState = (): AgentRuntimeState =>
  stateFrom([
    { type: "session_opened" },
    { type: "user_message_submitted", text: "question" },
    { type: "run_started" },
  ]);

export class ManualSnapshotFeed implements RuntimeSnapshotFeed {
  readonly #waiting: ((result: IteratorResult<AgentRuntimeState>) => void)[] = [];
  #latest: AgentRuntimeState | undefined;
  #closed = false;
  nextCalls = 0;
  closeCalls = 0;

  constructor(initial?: AgentRuntimeState) {
    this.#latest = initial;
  }

  [Symbol.asyncIterator](): AsyncIterableIterator<AgentRuntimeState> {
    return this;
  }

  next(): Promise<IteratorResult<AgentRuntimeState>> {
    this.nextCalls += 1;
    if (this.#closed) return Promise.resolve({ value: undefined, done: true });
    if (this.#latest !== undefined) {
      const value = this.#latest;
      this.#latest = undefined;
      return Promise.resolve({ value, done: false });
    }
    return new Promise((resolve) => this.#waiting.push(resolve));
  }

  publish(state: AgentRuntimeState): void {
    if (this.#closed) return;
    const waiting = this.#waiting.shift();
    if (waiting === undefined) this.#latest = state;
    else waiting({ value: state, done: false });
  }

  close(): void {
    if (this.#closed) return;
    this.#closed = true;
    this.closeCalls += 1;
    for (const waiting of this.#waiting.splice(0)) waiting({ value: undefined, done: true });
  }

  return(): Promise<IteratorResult<AgentRuntimeState>> {
    this.close();
    return Promise.resolve({ value: undefined, done: true });
  }

  throw(_error?: unknown): Promise<IteratorResult<AgentRuntimeState>> {
    this.close();
    return Promise.resolve({ value: undefined, done: true });
  }
}

export class FakeAgentRuntime implements AgentRuntime {
  readonly starts: string[] = [];
  readonly steering: string[] = [];
  readonly followUps: string[] = [];
  cancelCalls = 0;
  disposeCalls = 0;
  startError: Error | undefined;
  controlError: Error | undefined;
  readonly feed: ManualSnapshotFeed;

  constructor(public state: AgentRuntimeState) {
    this.feed = new ManualSnapshotFeed(state);
  }

  snapshot(): AgentRuntimeState {
    return this.state;
  }

  snapshots(): RuntimeSnapshotFeed {
    return this.feed;
  }

  async startTurn(input: string): Promise<void> {
    if (this.startError !== undefined) throw this.startError;
    this.starts.push(input);
  }

  async steer(input: string): Promise<void> {
    if (this.controlError !== undefined) throw this.controlError;
    this.steering.push(input);
  }

  async followUp(input: string): Promise<void> {
    if (this.controlError !== undefined) throw this.controlError;
    this.followUps.push(input);
  }

  cancel(): void {
    if (this.controlError !== undefined) throw this.controlError;
    this.cancelCalls += 1;
  }

  dispose(): Promise<void> {
    this.disposeCalls += 1;
    return Promise.resolve();
  }
}
