import { types } from "node:util";

import type { BudgetTracker } from "./budget.js";
import { kernelError } from "./errors.js";

const MODEL_EVENT_KEYS: ReadonlyMap<string, ReadonlySet<string>> = new Map([
  ["usage", new Set(["type", "usage"])],
  ["text_delta", new Set(["type", "delta"])],
  ["tool_call", new Set(["type", "callId", "name", "arguments"])],
  ["error", new Set(["type", "code", "message"])],
  ["done", new Set(["type", "reason"])],
]);

export function inspectRawModelEvent(event: unknown, tracker: BudgetTracker): unknown {
  if (typeof event !== "object" || event === null || Array.isArray(event)) return undefined;
  if (types.isProxy(event)) throw invalidModelEvent();
  const keys = Reflect.ownKeys(event);
  if (keys.length > 4 || keys.some((key) => typeof key !== "string")) throw invalidModelEvent();
  const typeDescriptor = Object.getOwnPropertyDescriptor(event, "type");
  const type: unknown =
    typeDescriptor && "value" in typeDescriptor ? typeDescriptor.value : undefined;
  const allowed = typeof type === "string" ? MODEL_EVENT_KEYS.get(type) : undefined;
  if (!allowed || keys.length !== allowed.size) throw invalidModelEvent();
  for (const key of keys) {
    if (typeof key !== "string" || !allowed.has(key)) throw invalidModelEvent();
  }
  const deltaDescriptor =
    type === "text_delta" ? Object.getOwnPropertyDescriptor(event, "delta") : undefined;
  const delta: unknown =
    deltaDescriptor && "value" in deltaDescriptor ? deltaDescriptor.value : undefined;
  if (type === "text_delta" && typeof delta === "string") tracker.output(delta);
  return type;
}

function invalidModelEvent(): Error {
  return kernelError("MODEL_ERROR", "Invalid model event denied");
}
