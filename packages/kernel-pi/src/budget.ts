import { kernelError } from "./errors.ts";
import { assertByteLimit } from "./sanitization.ts";
import type { KernelResourceBudget } from "./types.ts";

const BUDGET_KEYS = [
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
] as const satisfies readonly (keyof KernelResourceBudget)[];

export function validateBudget(budget: KernelResourceBudget): void {
  for (const key of BUDGET_KEYS) {
    const value = budget[key];
    if (!Number.isSafeInteger(value) || value <= 0) {
      throw new TypeError(`${key} must be a positive safe integer`);
    }
  }
  for (const key of ["maxSecretCount", "maxSecretBytes"] as const) {
    const value = budget[key];
    if (value !== undefined && (!Number.isSafeInteger(value) || value <= 0)) {
      throw new TypeError(`${key} must be a positive safe integer when provided`);
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
      throw kernelError("TURN_BUDGET_EXCEEDED", "Maximum model turns exceeded");
    }
    return this.#turns;
  }

  step(): void {
    this.#steps += 1;
    if (this.#steps > this.limits.maxSteps) {
      throw kernelError("STEP_BUDGET_EXCEEDED", "Maximum tool steps exceeded");
    }
  }

  output(value: string): void {
    this.#outputBytes += Buffer.byteLength(value, "utf8");
    if (this.#outputBytes > this.limits.maxOutputBytes) {
      throw kernelError("OUTPUT_BUDGET_EXCEEDED", "Maximum model output bytes exceeded");
    }
  }

  remainingOutputBytes(): number {
    return Math.max(0, this.limits.maxOutputBytes - this.#outputBytes);
  }
}
