const TASK_DRAIN_TIMEOUT_MS = 100;

export class RuntimeBackgroundTasks {
  readonly #controller = new AbortController();
  readonly #tasks = new Set<Promise<void>>();
  #closed = false;

  track(work: (signal: AbortSignal) => Promise<void>): boolean {
    if (this.#closed) return false;
    const task = Promise.resolve().then(async () => {
      if (!this.#closed) await work(this.#controller.signal);
    });
    this.#tasks.add(task);
    void task.then(
      () => this.#tasks.delete(task),
      () => this.#tasks.delete(task),
    );
    return true;
  }

  async close(options: {
    readonly cancel: () => void;
    readonly dispose: () => void;
    readonly cancelActive: boolean;
  }): Promise<boolean> {
    if (this.#closed) return false;
    this.#closed = true;
    this.#controller.abort();
    let failed = false;
    if (options.cancelActive) {
      try {
        options.cancel();
      } catch {
        failed = true;
      }
    }
    await waitBounded([...this.#tasks]);
    try {
      options.dispose();
    } catch {
      failed = true;
    }
    return failed;
  }
}

async function waitBounded(tasks: readonly Promise<void>[]): Promise<void> {
  if (tasks.length === 0) return;
  await new Promise<void>((resolve) => {
    let settled = false;
    const finish = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve();
    };
    const timer = setTimeout(finish, TASK_DRAIN_TIMEOUT_MS);
    timer.unref();
    void Promise.allSettled(tasks).then(finish);
  });
}
