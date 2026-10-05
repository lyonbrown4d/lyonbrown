export type ShutdownSignal = "SIGINT" | "SIGTERM";

export interface ShutdownSignalSource {
  once(signal: ShutdownSignal, listener: () => void): unknown;
  removeListener(signal: ShutdownSignal, listener: () => void): unknown;
}

export interface CloseableAgentd {
  close(): Promise<void>;
}

export function waitForShutdownSignal(
  running: CloseableAgentd,
  source: ShutdownSignalSource = process,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let triggered = false;
    const cleanup = (): void => {
      source.removeListener("SIGINT", shutdown);
      source.removeListener("SIGTERM", shutdown);
    };
    const shutdown = (): void => {
      if (triggered) return;
      triggered = true;
      cleanup();
      void running.close().then(resolve, reject);
    };
    source.once("SIGINT", shutdown);
    source.once("SIGTERM", shutdown);
  });
}
