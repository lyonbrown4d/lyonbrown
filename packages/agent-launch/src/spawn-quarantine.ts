import type { StageRunner } from "./stage-runner.js";
import type { SpawnAttempt, StartupLease } from "./types.js";

export class SpawnQuarantine {
  public constructor(private readonly stages: StageRunner) {}

  public start(lease: StartupLease, attempt: SpawnAttempt): void {
    const safe = Promise.race([
      this.#fulfillmentOnly(() => attempt.retired),
      this.#fulfillmentOnly(() => attempt.exited),
    ]);
    void safe
      .then(
        () => this.#releaseLease(lease),
        () => undefined,
      )
      .catch(() => undefined);
  }

  async #releaseLease(lease: StartupLease): Promise<void> {
    await this.stages.cleanup(async (signal) => {
      await lease.release(signal);
    });
  }

  #fulfillmentOnly(read: () => Promise<unknown>): Promise<void> {
    return new Promise<void>((resolve) => {
      let candidate: unknown;
      try {
        candidate = read();
      } catch {
        return;
      }
      if (
        (typeof candidate !== "object" || candidate === null) &&
        typeof candidate !== "function"
      ) {
        return;
      }
      let then: unknown;
      try {
        then = (candidate as { readonly then?: unknown }).then;
      } catch {
        return;
      }
      if (typeof then !== "function") return;
      const observed = Promise.resolve(candidate as PromiseLike<unknown>);
      void observed.then(
        () => {
          resolve();
        },
        () => undefined,
      );
    });
  }
}
