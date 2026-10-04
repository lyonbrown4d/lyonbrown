const REPLAY_CONFLICTS = new WeakSet<object>();
const REPLAY_CAPACITY_ERRORS = new WeakSet<object>();

export class ReplayConflictError extends Error {
  public override readonly name = "ReplayConflictError";
  public constructor() {
    super("command_id was already used for a different command payload");
    REPLAY_CONFLICTS.add(this);
  }
}

export class ReplayCapacityError extends Error {
  public override readonly name = "ReplayCapacityError";
  public constructor() {
    super("command replay capacity is exhausted by in-flight commands");
    REPLAY_CAPACITY_ERRORS.add(this);
  }
}

export function isReplayConflict(error: unknown): error is ReplayConflictError {
  return typeof error === "object" && error !== null && REPLAY_CONFLICTS.has(error);
}

export function isReplayCapacityError(error: unknown): error is ReplayCapacityError {
  return typeof error === "object" && error !== null && REPLAY_CAPACITY_ERRORS.has(error);
}

interface ReplayEntry<T> {
  readonly fingerprint: string;
  readonly promise: Promise<T>;
  completed: boolean;
}

export class ReplayCache<T> {
  readonly #capacity: number;
  readonly #entries = new Map<string, ReplayEntry<T>>();

  public constructor(capacity: number) {
    if (!Number.isSafeInteger(capacity) || capacity < 1) {
      throw new RangeError("replay capacity must be a positive safe integer");
    }
    this.#capacity = capacity;
  }

  public get size(): number {
    return this.#entries.size;
  }

  public execute(key: string, fingerprint: string, operation: () => Promise<T>): Promise<T> {
    const existing = this.#entries.get(key);
    if (existing !== undefined) {
      if (existing.fingerprint !== fingerprint) return Promise.reject(new ReplayConflictError());
      return existing.promise;
    }

    this.#evictCompletedUntilAvailable();
    if (this.#entries.size >= this.#capacity) return Promise.reject(new ReplayCapacityError());

    let resolveOperation: (value: T | PromiseLike<T>) => void = () => undefined;
    let rejectOperation: (reason?: unknown) => void = () => undefined;
    const promise = new Promise<T>((resolve, reject) => {
      resolveOperation = resolve;
      rejectOperation = reject;
    });
    const entry: ReplayEntry<T> = { fingerprint, completed: false, promise };
    this.#entries.set(key, entry);
    try {
      void operation().then(resolveOperation, rejectOperation);
    } catch (error) {
      rejectOperation(error);
    }
    void entry.promise.then(
      () => {
        entry.completed = true;
      },
      () => {
        entry.completed = true;
      },
    );
    return promise;
  }

  #evictCompletedUntilAvailable(): void {
    for (const [key, entry] of this.#entries) {
      if (this.#entries.size < this.#capacity) return;
      if (entry.completed) this.#entries.delete(key);
    }
  }
}
