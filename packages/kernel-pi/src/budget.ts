import { KernelRuntimeError } from "./errors.js";
import { assertByteLimit } from "./sanitization.js";
import type { KernelResourceBudget } from "./types.js";

const BUDGET_KEYS: readonly (keyof KernelResourceBudget)[] = [
  "maxInputBytes",
  "maxTurns",
  "maxSteps",
  "maxOutputBytes",
  "maxToolResultBytes",
  "maxValueDepth",
  "maxValueStringBytes",
  "maxCollectionEntries",
  "maxValueNodes",
  "toolTimeoutMs",
];

export function validateBudget(budget: KernelResourceBudget): void {
  for (const key of BUDGET_KEYS) {
    const value = budget[key];
    if (!Number.isSafeInteger(value) || value <= 0) {
      throw new TypeError(`${key} must be a positive safe integer`);
    }
  }
}

export class BudgetTracker {
  #turns = 0;
  #steps = 0;
  #outputBytes = 0;

  constructor(readonly limits: KernelResourceBudget) {}

  input(value: string): void {
    assertByteLimit(value, this.limits.maxInputBytes, "INPUT_BUDGET_EXCEEDED");
  }

  turn(): number {
    this.#turns += 1;
    if (this.#turns > this.limits.maxTurns) {
      throw new KernelRuntimeError("TURN_BUDGET_EXCEEDED", "Maximum model turns exceeded");
    }
    return this.#turns;
  }

  step(): void {
    this.#steps += 1;
    if (this.#steps > this.limits.maxSteps) {
      throw new KernelRuntimeError("STEP_BUDGET_EXCEEDED", "Maximum tool steps exceeded");
    }
  }

  output(value: string): void {
    this.#outputBytes += Buffer.byteLength(value, "utf8");
    if (this.#outputBytes > this.limits.maxOutputBytes) {
      throw new KernelRuntimeError("OUTPUT_BUDGET_EXCEEDED", "Maximum model output bytes exceeded");
    }
  }

  remainingOutputBytes(): number {
    return Math.max(0, this.limits.maxOutputBytes - this.#outputBytes);
  }
}
