import {
  assertProgress,
  bounded,
  errorFromUnknown,
  waitForCondition,
  type ProgressState,
} from "./smoke-real-gguf-lib.ts";
import {
  assertSmokeSnapshotHealthy,
  hasProgress,
  isActiveDownload,
  isReady,
  isSetup,
  requireManagedModel,
  requireOperationId,
  requireSessionId,
} from "./smoke-real-gguf-assertions.ts";

export interface SmokeSnapshot {
  readonly revision: bigint;
  readonly phase: string;
  readonly sessionId: string | null;
  readonly modelSetup: {
    readonly phase: string;
    readonly operationId: string | null;
    readonly downloadedBytes: bigint;
    readonly totalBytes: bigint | null;
    readonly managedModel: {
      readonly id: string;
      readonly maxBytes: bigint;
      readonly sha256: string;
    } | null;
    readonly allowedActions: readonly string[];
    readonly failure: { readonly code: string } | null;
  } | null;
  readonly turns: readonly {
    readonly userText: string;
    readonly status: string;
    readonly assistantText: string;
    readonly activities: readonly {
      readonly phase: string;
      readonly usage: { readonly input: number; readonly output: number } | null;
    }[];
  }[];
}

export interface RealGgufSmokePort {
  readSnapshot(): Promise<SmokeSnapshot>;
  startDownload(modelId: string): Promise<void>;
  cancelDownload(operationId: string): Promise<void>;
  startTurn(text: string): Promise<void>;
  cancelTurn(): Promise<void>;
  closeServer(): Promise<void>;
  restartServer(): Promise<void>;
  verifyInstalledCache(expectedSha256: string): Promise<void>;
  cleanup(): Promise<void>;
}

export interface RealGgufSmokeFlowOptions {
  readonly phaseTimeoutMs: number;
  readonly downloadTimeoutMs: number;
  readonly pollIntervalMs: number;
  readonly cancelAfterBytes: bigint;
  readonly closeTimeoutMs: number;
  readonly expectedSha256: string;
}

const SHORT_PROMPT = "Respond with exactly one short sentence confirming the smoke test.";
const LONG_PROMPT =
  "Write a detailed numbered explanation with at least 500 items. Continue until cancelled.";

export async function runRealGgufSmokeFlow(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
): Promise<void> {
  let primaryFailure: unknown;
  try {
    const initial = await waitSnapshot(port, options, "setup-required", (snapshot) =>
      isSetup(snapshot, "required"),
    );
    const model = requireManagedModel(initial);
    if (model.sha256 !== options.expectedSha256) {
      throw new Error("REAL_GGUF_CATALOG_DIGEST_MISMATCH");
    }
    const initialSetup = initial.modelSetup;
    if (
      initialSetup?.operationId !== null ||
      initialSetup.downloadedBytes !== 0n ||
      initialSetup.failure !== null ||
      !initialSetup.allowedActions.includes("download")
    ) {
      throw new Error("REAL_GGUF_UNEXPECTED_AUTO_DOWNLOAD");
    }

    await port.startDownload(model.id);
    const firstProgress = await waitSnapshot(port, options, "first-download-progress", (snapshot) =>
      hasProgress(snapshot, options.cancelAfterBytes),
    );
    const firstOperation = requireOperationId(firstProgress);
    await port.cancelDownload(firstOperation);
    await waitSnapshot(port, options, "download-cancelled", (snapshot) => {
      const setup = snapshot.modelSetup;
      return (
        setup?.phase === "cancelled" &&
        setup.operationId === firstOperation &&
        setup.allowedActions.includes("retry")
      );
    });

    await port.startDownload(model.id);
    let progress: ProgressState | undefined;
    let secondOperation = "";
    const ready = await waitSnapshot(
      port,
      { ...options, phaseTimeoutMs: options.downloadTimeoutMs },
      "download-ready",
      isReady,
      (snapshot) => {
        const setup = snapshot.modelSetup;
        if (!setup?.operationId || !isActiveDownload(setup)) return;
        if (secondOperation.length === 0) {
          secondOperation = setup.operationId;
          if (secondOperation === firstOperation)
            throw new Error("REAL_GGUF_OPERATION_NOT_RENEWED");
          progress = {
            operationId: secondOperation,
            downloadedBytes: 0n,
            totalBytes: null,
            maxBytes: model.maxBytes,
          };
        }
        if (setup.operationId !== secondOperation || progress === undefined) return;
        if (setup.phase === "downloading") {
          progress = assertProgress(progress, setup.downloadedBytes, setup.totalBytes);
        }
      },
    );
    if (secondOperation.length === 0 || progress === undefined || progress.downloadedBytes <= 0n) {
      throw new Error("REAL_GGUF_PROGRESS_NOT_OBSERVED");
    }

    await completeShortTurn(port, options);
    await cancelStreamingTurn(port, options);
    const completedBeforeRestart = await completeShortTurn(port, options);
    const previousSessionId = requireSessionId(completedBeforeRestart);
    if (requireManagedModel(ready).sha256 !== options.expectedSha256) {
      throw new Error("REAL_GGUF_READY_DIGEST_MISMATCH");
    }
    await bounded("first-close", options.closeTimeoutMs, port.closeServer());
    await port.verifyInstalledCache(options.expectedSha256);

    await port.restartServer();
    await waitSnapshot(port, options, "cached-restart-ready", (snapshot) => {
      return (
        isReady(snapshot) &&
        snapshot.turns.length === 0 &&
        snapshot.sessionId !== null &&
        snapshot.sessionId !== previousSessionId
      );
    });
    await completeShortTurn(port, options);
    await startStreamingTurn(port, options);
    await bounded("active-inference-close", options.closeTimeoutMs, port.closeServer());
  } catch (error: unknown) {
    primaryFailure = error;
  }

  try {
    await port.cleanup();
  } catch (cleanupFailure: unknown) {
    if (primaryFailure !== undefined) {
      throw new AggregateError(
        [primaryFailure, cleanupFailure],
        "real GGUF smoke and cleanup failed",
        { cause: primaryFailure },
      );
    }
    throw errorFromUnknown(cleanupFailure);
  }
  if (primaryFailure !== undefined) throw errorFromUnknown(primaryFailure);
}

async function completeShortTurn(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
): Promise<SmokeSnapshot> {
  const baseline = (await port.readSnapshot()).turns.length;
  await port.startTurn(SHORT_PROMPT);
  const snapshot = await waitSnapshot(port, options, "short-turn", (value) => {
    const turn = value.turns.at(-1);
    return (
      value.phase === "completed" &&
      value.turns.length > baseline &&
      turn?.userText === SHORT_PROMPT &&
      turn.status === "completed"
    );
  });
  const turn = snapshot.turns.at(-1);
  const activity = turn?.activities.at(-1);
  const usage = activity?.usage;
  if (
    turn === undefined ||
    turn.assistantText.trim().length === 0 ||
    turn.userText !== SHORT_PROMPT ||
    activity?.phase !== "completed" ||
    usage === null ||
    usage === undefined ||
    usage.input <= 0 ||
    usage.output < 0
  ) {
    throw new Error("REAL_GGUF_SHORT_TURN_INVALID");
  }
  return snapshot;
}

async function cancelStreamingTurn(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
): Promise<void> {
  const baseline = await startStreamingTurn(port, options);
  await port.cancelTurn();
  await waitSnapshot(port, options, "turn-cancelled", (snapshot) => {
    const turn = snapshot.turns.at(-1);
    return (
      snapshot.phase === "ready" &&
      snapshot.turns.length > baseline &&
      turn?.userText === LONG_PROMPT &&
      turn.status === "cancelled"
    );
  });
}

async function startStreamingTurn(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
): Promise<number> {
  const baseline = (await port.readSnapshot()).turns.length;
  await port.startTurn(LONG_PROMPT);
  await waitSnapshot(port, options, "turn-streaming", (snapshot) => {
    const turn = snapshot.turns.at(-1);
    return (
      (snapshot.phase === "preparing" || snapshot.phase === "streaming") &&
      snapshot.turns.length > baseline &&
      turn?.userText === LONG_PROMPT &&
      turn.status === "running" &&
      turn.activities.length > 0
    );
  });
  return baseline;
}

function waitSnapshot(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
  label: string,
  accept: (snapshot: SmokeSnapshot) => boolean,
  observe?: (snapshot: SmokeSnapshot) => void,
): Promise<SmokeSnapshot> {
  return waitForCondition({
    label,
    timeoutMs: options.phaseTimeoutMs,
    intervalMs: options.pollIntervalMs,
    read: async () => {
      const snapshot = await port.readSnapshot();
      assertSmokeSnapshotHealthy(snapshot);
      return snapshot;
    },
    accept,
    ...(observe === undefined ? {} : { observe }),
  });
}
