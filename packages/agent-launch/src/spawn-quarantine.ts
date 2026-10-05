import type { StageRunner } from "./stage-runner.ts";
import type { SpawnAttempt, StartupLease } from "./types.ts";

interface QuarantineEntry {
  readonly lease: StartupLease;
  readonly attempt: SpawnAttempt;
  readonly retired: Promise<void>;
  readonly exited: Promise<void>;
  readonly terminal: Promise<void>;
}

export class SpawnQuarantine {
  readonly #entries = new Set<QuarantineEntry>();

  public constructor(private readonly stages: StageRunner) {}

  public start(lease: StartupLease, attempt: SpawnAttempt): void {
    const retired = this.#trustedFulfillment(() => attempt.retired);
    const exited = this.#trustedFulfillment(() => attempt.exited);
    const entry: QuarantineEntry = {
      lease,
      attempt,
      retired,
      exited,
      terminal: Promise.race([retired, exited]),
    };
    this.#entries.add(entry);
    void entry.terminal
      .then(
        () => this.#releaseLease(entry),
        () => undefined,
      )
      .catch(() => undefined);
  }

  /** Package-internal test probe; this class is not reachable from package exports. */
  public retainedCountForTesting(): number {
    return this.#entries.size;
  }

  async #releaseLease(entry: QuarantineEntry): Promise<void> {
    const released = await this.stages.cleanup(async (signal) => {
      await entry.lease.release(signal);
    });
    if (released) this.#entries.delete(entry);
  }

  #trustedFulfillment(read: () => Promise<unknown>): Promise<void> {
    return new Promise<void>((resolve) => {
      let candidate: unknown;
      try {
        candidate = read();
      } catch {
        return;
      }
      try {
        void Promise.prototype.then.call(
          candidate as Promise<unknown>,
          () => {
            resolve();
          },
          () => undefined,
        );
      } catch {
        return;
      }
    });
  }
}
