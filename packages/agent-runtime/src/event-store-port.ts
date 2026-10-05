import { types as nodeTypes } from "node:util";

import type { EventStore, LoadOptions, StoredEvent } from "@lyonbrown/event-store";

import { runtimeError } from "./errors.ts";

const runtimeEventPageBrand: unique symbol = Symbol("runtime-event-page");
const pageContents = new WeakMap<object, readonly StoredEvent[]>();

export interface RuntimeEventPage {
  readonly [runtimeEventPageBrand]: true;
}

export type RuntimeEventStoreSource = Pick<EventStore, "append" | "currentRevision" | "load">;

export interface RuntimeEventStorePort {
  append: EventStore["append"];
  currentRevision: EventStore["currentRevision"];
  loadPage(sessionId: string, options: LoadOptions): Promise<RuntimeEventPage>;
}

export function createRuntimeEventPage(value: unknown, requestedLimit: number): RuntimeEventPage {
  const events = copyDenseEventPage(value, requestedLimit);
  const page = Object.freeze({ [runtimeEventPageBrand]: true }) as RuntimeEventPage;
  pageContents.set(page, events);
  return page;
}

export function readRuntimeEventPage(
  page: RuntimeEventPage,
  requestedLimit: number,
): readonly StoredEvent[] {
  const events = pageContents.get(page);
  if (events === undefined || events.length > requestedLimit) throw streamCorrupt();
  return events;
}

export function createRuntimeEventStorePort(store: RuntimeEventStoreSource): RuntimeEventStorePort {
  return {
    append: store.append.bind(store),
    currentRevision: store.currentRevision.bind(store),
    loadPage: async (sessionId, options) =>
      createRuntimeEventPage(await store.load(sessionId, options), options.limit),
  };
}

function copyDenseEventPage(value: unknown, limit: number): readonly StoredEvent[] {
  if (typeof value !== "object" || value === null || nodeTypes.isProxy(value)) {
    throw streamCorrupt();
  }
  if (!Array.isArray(value) || Reflect.getPrototypeOf(value) !== Array.prototype) {
    throw streamCorrupt();
  }
  const lengthDescriptor = Reflect.getOwnPropertyDescriptor(value, "length");
  if (
    lengthDescriptor === undefined ||
    !("value" in lengthDescriptor) ||
    !Number.isSafeInteger(lengthDescriptor.value) ||
    lengthDescriptor.value < 0 ||
    lengthDescriptor.value > limit
  ) {
    throw streamCorrupt();
  }
  const length = lengthDescriptor.value;
  if (Reflect.ownKeys(value).length !== length + 1) throw streamCorrupt();
  const events: StoredEvent[] = [];
  for (let index = 0; index < length; index += 1) {
    const descriptor = Reflect.getOwnPropertyDescriptor(value, String(index));
    if (descriptor === undefined || !("value" in descriptor) || !descriptor.enumerable) {
      throw streamCorrupt();
    }
    events.push(descriptor.value as StoredEvent);
  }
  return Object.freeze(events);
}

function streamCorrupt() {
  return runtimeError("EVENT_STREAM_CORRUPT", "Durable runtime event stream is invalid");
}
