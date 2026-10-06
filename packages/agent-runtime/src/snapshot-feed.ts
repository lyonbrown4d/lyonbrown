import type { AgentRuntimeState } from "@lyonbrown/agent-domain";

import { runtimeError } from "@lyonbrown/agent-runtime/internal/errors";
import type { RuntimeSnapshotFeed } from "@lyonbrown/agent-runtime/internal/types";

type PendingRead = (result: IteratorResult<AgentRuntimeState>) => void;

export class LatestSnapshotFeed implements RuntimeSnapshotFeed {
  #latest: AgentRuntimeState | undefined;
  #pendingRead: PendingRead | undefined;
  #closed = false;

  constructor(
    initial: AgentRuntimeState,
    private readonly onClose: () => void = () => undefined,
  ) {
    this.#latest = initial;
  }

  [Symbol.asyncIterator](): AsyncIterableIterator<AgentRuntimeState> {
    return this;
  }

  next(): Promise<IteratorResult<AgentRuntimeState>> {
    if (this.#latest !== undefined) {
      const value = this.#latest;
      this.#latest = undefined;
      return Promise.resolve({ value, done: false });
    }
    if (this.#closed) return Promise.resolve({ value: undefined, done: true });
    if (this.#pendingRead !== undefined) {
      return Promise.reject(
        runtimeError("SESSION_BUSY", "Snapshot feed already has a pending read"),
      );
    }
    return new Promise<IteratorResult<AgentRuntimeState>>((resolve) => {
      this.#pendingRead = resolve;
    });
  }

  return(): Promise<IteratorResult<AgentRuntimeState>> {
    this.close();
    return Promise.resolve({ value: undefined, done: true });
  }

  throw(error?: unknown): Promise<IteratorResult<AgentRuntimeState>> {
    this.close();
    return Promise.reject(
      error instanceof Error
        ? error
        : runtimeError("RUNTIME_DISPOSED", "Snapshot feed was stopped by its consumer"),
    );
  }

  publish(snapshot: AgentRuntimeState): void {
    if (this.#closed) return;
    const pending = this.#pendingRead;
    if (pending === undefined) {
      this.#latest = snapshot;
      return;
    }
    this.#pendingRead = undefined;
    pending({ value: snapshot, done: false });
  }

  close(): void {
    if (this.#closed) return;
    this.#closed = true;
    this.#latest = undefined;
    const pending = this.#pendingRead;
    this.#pendingRead = undefined;
    pending?.({ value: undefined, done: true });
    this.onClose();
  }
}
