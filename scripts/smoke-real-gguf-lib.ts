import { realpath } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname } from "node:path";

export interface ProgressState {
  readonly operationId: string;
  readonly downloadedBytes: bigint;
  readonly totalBytes: bigint | null;
  readonly maxBytes: bigint;
}

export interface WaitOptions<T> {
  readonly label: string;
  readonly timeoutMs: number;
  readonly read: () => Promise<T>;
  readonly accept: (value: T) => boolean;
  readonly observe?: (value: T) => void;
  readonly intervalMs?: number;
}

type Environment = Readonly<Record<string, string | undefined>>;

export function parseRealGgufSmokeAuthorization(
  environment: Environment,
  arguments_: readonly string[],
  platform: NodeJS.Platform = process.platform,
): { readonly confirmed: true } {
  if (environment.LYONBROWN_REAL_GGUF_SMOKE !== "1") {
    throw new Error("REAL_GGUF_SMOKE_DISABLED");
  }
  if (!arguments_.includes("--confirm-download")) {
    throw new Error("REAL_GGUF_DOWNLOAD_NOT_CONFIRMED");
  }
  if (!["win32", "darwin", "linux"].includes(platform)) {
    throw new Error("REAL_GGUF_SMOKE_UNSUPPORTED_PLATFORM");
  }
  return { confirmed: true };
}

export function assertProgress(
  previous: ProgressState,
  downloadedBytes: bigint,
  totalBytes: bigint | null,
): ProgressState {
  const previousTotal = previous.totalBytes ?? 0n;
  const nextTotal = totalBytes ?? previousTotal;
  if (
    downloadedBytes < previous.downloadedBytes ||
    downloadedBytes < 0n ||
    nextTotal < previousTotal ||
    nextTotal < downloadedBytes ||
    downloadedBytes > previous.maxBytes ||
    nextTotal > previous.maxBytes
  ) {
    throw new Error("REAL_GGUF_PROGRESS_INVALID");
  }
  return {
    ...previous,
    downloadedBytes,
    totalBytes: totalBytes ?? previous.totalBytes,
  };
}

export async function waitForCondition<T>(options: WaitOptions<T>): Promise<T> {
  if (!Number.isSafeInteger(options.timeoutMs) || options.timeoutMs < 1) {
    throw new RangeError("timeoutMs must be a positive safe integer");
  }
  const intervalMs = options.intervalMs ?? 100;
  if (!Number.isSafeInteger(intervalMs) || intervalMs < 1 || intervalMs > options.timeoutMs) {
    throw new RangeError("intervalMs is invalid");
  }
  const deadline = performance.now() + options.timeoutMs;
  let latest = await options.read();
  options.observe?.(latest);
  while (!options.accept(latest)) {
    const remaining = deadline - performance.now();
    if (remaining <= 0) throw new Error(`REAL_GGUF_TIMEOUT:${options.label}`);
    await abortableDelay(Math.min(intervalMs, remaining));
    latest = await options.read();
    options.observe?.(latest);
  }
  return latest;
}

export async function bounded<T>(label: string, timeoutMs: number, task: Promise<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error(`REAL_GGUF_TIMEOUT:${label}`));
    }, timeoutMs);
    void task.then(
      (value) => {
        clearTimeout(timeout);
        resolve(value);
      },
      (error: unknown) => {
        clearTimeout(timeout);
        reject(errorFromUnknown(error));
      },
    );
  });
}

export function errorFromUnknown(error: unknown): Error {
  return error instanceof Error ? error : new Error("REAL_GGUF_UNKNOWN_FAILURE");
}

export async function runAllAttemptCleanup(
  cleanup: readonly (() => void | Promise<void>)[],
): Promise<void> {
  const failures: unknown[] = [];
  for (const operation of cleanup) {
    try {
      await operation();
    } catch (error: unknown) {
      failures.push(error);
    }
  }
  if (failures.length > 0) throw new AggregateError(failures, "real GGUF cleanup failed");
}

export async function assertSafeTemporaryRoot(path: string): Promise<void> {
  const [resolvedRoot, resolvedTemporaryDirectory] = await Promise.all([
    realpath(path),
    realpath(tmpdir()),
  ]);
  if (
    dirname(resolvedRoot) !== resolvedTemporaryDirectory ||
    !basename(resolvedRoot).startsWith("lyonbrown-real-gguf-")
  ) {
    throw new Error("REAL_GGUF_CLEANUP_ROOT_INVALID");
  }
}

export async function cleanupTemporaryAfterClose(
  close: () => Promise<void>,
  remove: () => Promise<void>,
): Promise<void> {
  await close();
  await remove();
}

function abortableDelay(durationMs: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, durationMs);
  });
}
