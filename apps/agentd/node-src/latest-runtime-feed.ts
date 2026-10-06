import type { RuntimeEvent } from "@lyonbrown/protocol";

export interface RuntimeFeed {
  close(): void;
  publish(event: RuntimeEvent): void;
  next(): Promise<IteratorResult<RuntimeEvent>>;
}

export function latestRuntimeFeed(): RuntimeFeed {
  let pending: RuntimeEvent | undefined;
  let waiter: ((result: IteratorResult<RuntimeEvent>) => void) | undefined;
  let closed = false;
  return {
    publish(event) {
      if (closed) return;
      if (waiter !== undefined) {
        const resolve = waiter;
        waiter = undefined;
        resolve({ done: false, value: event });
      } else pending = event;
    },
    close() {
      if (closed) return;
      closed = true;
      const resolve = waiter;
      waiter = undefined;
      resolve?.({ done: true, value: undefined });
    },
    next() {
      if (pending !== undefined) {
        const value = pending;
        pending = undefined;
        return Promise.resolve({ done: false, value });
      }
      if (closed) return Promise.resolve({ done: true, value: undefined });
      return new Promise((resolve) => {
        waiter = resolve;
      });
    },
  };
}
