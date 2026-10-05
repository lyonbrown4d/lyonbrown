import type { ProcessSpawner, SpawnAgentdRequest, SpawnAttempt } from "../../src/types.ts";
import type { Candidate, FakeDiscovery } from "./fixtures.ts";

function rejectUnknown<T>(value: unknown): Promise<T> {
  return Promise.resolve().then(() => {
    throw value;
  });
}

export class FakeSpawner implements ProcessSpawner {
  public calls: SpawnAgentdRequest[] = [];
  public candidateToPublish: Candidate | null = null;
  public exit = new Promise<{ readonly exitCode: number | null }>(() => undefined);
  public failure: unknown;
  public abandonFailure: unknown;
  public abandonNeverSettles = false;

  public constructor(private readonly discovery: FakeDiscovery) {}

  public begin(request: SpawnAgentdRequest): SpawnAttempt {
    this.calls.push(request);
    if (this.failure !== undefined) {
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- hostile adapter boundary.
      throw this.failure;
    }
    let retire!: () => void;
    const retired = new Promise<void>((resolve) => {
      retire = resolve;
    });
    const attempt: SpawnAttempt = {
      exited: this.exit,
      retired,
      abandon: (signal) => {
        void signal;
        if (this.abandonFailure !== undefined) return rejectUnknown(this.abandonFailure);
        if (this.abandonNeverSettles) return new Promise<void>(() => undefined);
        retire();
        return Promise.resolve();
      },
    };
    queueMicrotask(() => {
      if (this.candidateToPublish) this.discovery.candidate = this.candidateToPublish;
    });
    return attempt;
  }
}
