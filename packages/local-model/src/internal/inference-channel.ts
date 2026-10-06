import type { LocalModelInferenceEvent } from "@lyonbrown/local-model/internal/types";

const MAX_QUEUED_EVENTS = 32;

export const singleInferenceEventIterator = (
  event: LocalModelInferenceEvent,
): AsyncIterator<LocalModelInferenceEvent> => {
  let pending: LocalModelInferenceEvent | undefined = event;
  return {
    next(): Promise<IteratorResult<LocalModelInferenceEvent, undefined>> {
      if (pending === undefined) return Promise.resolve({ value: undefined, done: true });
      const value = pending;
      pending = undefined;
      return Promise.resolve({ value, done: false });
    },
    return(): Promise<IteratorResult<LocalModelInferenceEvent, undefined>> {
      pending = undefined;
      return Promise.resolve({ value: undefined, done: true });
    },
  };
};

export class InferenceEventChannel {
  private readonly queued: LocalModelInferenceEvent[] = [];
  private pending:
    | ((result: IteratorResult<LocalModelInferenceEvent, undefined>) => void)
    | undefined;
  private closed = false;

  push(event: LocalModelInferenceEvent): void {
    if (this.closed) return;
    if (this.pending !== undefined) {
      const resolve = this.pending;
      this.pending = undefined;
      resolve({ value: event, done: false });
      return;
    }
    if (event.type === "text_delta" && this.queued.length >= MAX_QUEUED_EVENTS) {
      const previous = this.queued.at(-1);
      if (previous?.type === "text_delta") {
        this.queued[this.queued.length - 1] = {
          type: "text_delta",
          text: previous.text + event.text,
        };
        return;
      }
    }
    this.makeTerminalRoom();
    this.queued.push(event);
  }

  close(): void {
    this.closed = true;
    if (this.pending === undefined) return;
    const resolve = this.pending;
    this.pending = undefined;
    resolve({ value: undefined, done: true });
  }

  cancel(): void {
    this.queued.splice(0, this.queued.length);
    this.close();
  }

  next(): Promise<IteratorResult<LocalModelInferenceEvent, undefined>> {
    const event = this.queued.shift();
    if (event !== undefined) return Promise.resolve({ value: event, done: false });
    if (this.closed) return Promise.resolve({ value: undefined, done: true });
    return new Promise((resolve) => {
      this.pending = resolve;
    });
  }

  private makeTerminalRoom(): void {
    if (this.queued.length < MAX_QUEUED_EVENTS) return;
    const text = this.queued
      .filter(
        (event): event is Extract<LocalModelInferenceEvent, { type: "text_delta" }> =>
          event.type === "text_delta",
      )
      .map((event) => event.text)
      .join("");
    const nonText = this.queued.filter((event) => event.type !== "text_delta");
    const combined: LocalModelInferenceEvent[] = text === "" ? [] : [{ type: "text_delta", text }];
    this.queued.splice(0, this.queued.length, ...combined, ...nonText);
  }
}
