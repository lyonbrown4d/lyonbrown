import { launchError } from "@lyonbrown/agent-launch/internal/errors";
import type { Clock, Sleeper } from "@lyonbrown/agent-launch/internal/types";

export type TimeoutCode = "STARTUP_TIMEOUT" | "READINESS_TIMEOUT";
export type DiscardValue<T> = (value: T, signal: AbortSignal) => Promise<void>;

export class StageRunner {
  public constructor(
    private readonly clock: Clock,
    private readonly sleeper: Sleeper,
    private readonly pollIntervalMs: number,
    private readonly cleanupMs: number,
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
    start: (signal: AbortSignal) => Promise<T>,
    deadline: number,
    code: TimeoutCode,
    signal?: AbortSignal,
    discard?: DiscardValue<T>,
  ): Promise<T> {
    this.throwIfAborted(signal);
    const remaining = deadline - this.clock.now();
    if (remaining <= 0) this.throwIfExpired(deadline, code);
    const operationController = new AbortController();
    const timerController = new AbortController();
    let accepted = false;
    const abortStage = () => {
      if (!accepted) operationController.abort();
      timerController.abort();
    };
    signal?.addEventListener("abort", abortStage, { once: true });
    const operation = this.#start(start, operationController.signal);
    const task = operation.then(
      (value) => ({ kind: "value" as const, value }),
      (error: unknown) => ({ kind: "error" as const, error }),
    );
    const timer = this.#start(
      (timerSignal) => this.sleeper.sleep(remaining, timerSignal),
      timerController.signal,
    ).then(
      () => ({ kind: "timeout" as const }),
      () => ({ kind: "timer-error" as const }),
    );
    try {
      const outcome = await Promise.race([task, timer]);
      this.throwIfAborted(signal);
      if (outcome.kind === "value") {
        accepted = true;
        return outcome.value;
      }
      if (outcome.kind === "error") throw outcome.error;
      if (outcome.kind === "timer-error") {
        throw launchError("unavailable", "CLIENT_FAILED", "deadline timer failed");
      }
      throw launchError("unavailable", code, "daemon startup timed out");
    } finally {
      signal?.removeEventListener("abort", abortStage);
      if (!accepted) operationController.abort();
      timerController.abort();
      if (!accepted && discard) {
        void operation.then(
          (value) => this.cleanup((cleanupSignal) => discard(value, cleanupSignal)),
          () => undefined,
        );
      }
    }
  }

  public async cleanup(start: (signal: AbortSignal) => Promise<void>): Promise<boolean> {
    const cleanupController = new AbortController();
    const timerController = new AbortController();
    const task = this.#start(start, cleanupController.signal).then(
      () => ({ kind: "settled" as const }),
      () => ({ kind: "failed" as const }),
    );
    const timer = this.#start(
      (timerSignal) => this.sleeper.sleep(this.cleanupMs, timerSignal),
      timerController.signal,
    ).then(
      () => ({ kind: "timeout" as const }),
      () => ({ kind: "timer-failed" as const }),
    );
    try {
      const outcome = await Promise.race([task, timer]);
      return outcome.kind === "settled";
    } finally {
      cleanupController.abort();
      timerController.abort();
    }
  }

  async #start<T>(start: (signal: AbortSignal) => Promise<T>, signal: AbortSignal): Promise<T> {
    return await start(signal);
  }
}
