import { launchError } from "./errors.js";
import type { Clock, Sleeper } from "./types.js";

export type TimeoutCode = "STARTUP_TIMEOUT" | "READINESS_TIMEOUT";

export class StageRunner {
  public constructor(
    private readonly clock: Clock,
    private readonly sleeper: Sleeper,
    private readonly pollIntervalMs: number,
  ) {}

  public now(): number {
    return this.clock.now();
  }

  public throwIfAborted(signal?: AbortSignal): void {
    if (signal?.aborted) {
      throw launchError("cancelled", "CANCELLED", "daemon connection was cancelled");
    }
  }

  public throwIfExpired(deadline: number, code: TimeoutCode): void {
    if (this.clock.now() < deadline) return;
    throw launchError("unavailable", code, "daemon startup timed out");
  }

  public async poll(deadline: number, code: TimeoutCode, signal?: AbortSignal): Promise<void> {
    this.throwIfAborted(signal);
    const remaining = deadline - this.clock.now();
    if (remaining <= 0) this.throwIfExpired(deadline, code);
    try {
      await this.sleeper.sleep(Math.min(this.pollIntervalMs, remaining), signal);
    } catch {
      this.throwIfAborted(signal);
      throw launchError("unavailable", "CLIENT_FAILED", "startup wait failed");
    }
    this.throwIfAborted(signal);
  }

  public async waitFor<T>(
    operation: Promise<T>,
    deadline: number,
    code: TimeoutCode,
    signal?: AbortSignal,
  ): Promise<T> {
    this.throwIfAborted(signal);
    const remaining = deadline - this.clock.now();
    if (remaining <= 0) this.throwIfExpired(deadline, code);
    const timerController = new AbortController();
    const abortTimer = () => {
      timerController.abort();
    };
    signal?.addEventListener("abort", abortTimer, { once: true });
    const task = operation.then(
      (value) => ({ kind: "value" as const, value }),
      (error: unknown) => ({ kind: "error" as const, error }),
    );
    const timer = this.sleeper.sleep(remaining, timerController.signal).then(
      () => ({ kind: "timeout" as const }),
      () => ({ kind: "timer-error" as const }),
    );
    try {
      const outcome = await Promise.race([task, timer]);
      this.throwIfAborted(signal);
      if (outcome.kind === "value") return outcome.value;
      if (outcome.kind === "error") throw outcome.error;
      if (outcome.kind === "timer-error") {
        throw launchError("unavailable", "CLIENT_FAILED", "deadline timer failed");
      }
      throw launchError("unavailable", code, "daemon startup timed out");
    } finally {
      signal?.removeEventListener("abort", abortTimer);
      timerController.abort();
    }
  }
}
