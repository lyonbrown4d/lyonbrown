import { fork, type ForkOptions } from "node:child_process";

import { DECISION_MODEL_TOTAL_BYTES } from "@lyonbrown/agentd/internal/decision-model-state";

export interface DecisionInstallProgress {
  readonly phase: "checking" | "waiting-for-lock" | "downloading" | "verifying" | "installing";
  readonly downloadedBytes?: number;
  readonly totalBytes?: number;
}

export function runDecisionInstallProcess(
  root: string,
  operationId: string,
  signal: AbortSignal,
  onProgress: (progress: DecisionInstallProgress) => void,
): Promise<void> {
  if (signal.aborted) return Promise.reject(new Error("decision_model_install_cancelled"));
  return new Promise<void>((resolve, reject) => {
    const options: ForkOptions & { readonly windowsHide: true } = {
      execArgv: [],
      stdio: ["ignore", "ignore", "ignore", "ipc"],
      windowsHide: true,
    };
    const child = fork(
      new URL("decision-install-worker.js", import.meta.url),
      [root, operationId],
      options,
    );
    let completed = false;
    let invalid = false;
    let lastBytes = 0;
    let killDeadline: NodeJS.Timeout | undefined;
    const cancel = (): void => {
      if (child.connected) child.send({ type: "cancel" }, () => undefined);
      killDeadline ??= setTimeout(() => {
        child.kill();
      }, 15_000);
      killDeadline.unref();
    };
    const cleanup = (): void => {
      signal.removeEventListener("abort", cancel);
      if (killDeadline !== undefined) clearTimeout(killDeadline);
    };
    child.on("message", (value: unknown) => {
      if (invalid) return;
      if (isCompleted(value)) {
        completed = true;
        return;
      }
      const progress = parseDecisionInstallProgress(value);
      if (
        progress === null ||
        (progress.downloadedBytes !== undefined && progress.downloadedBytes < lastBytes)
      ) {
        invalid = true;
        cancel();
        return;
      }
      lastBytes = progress.downloadedBytes ?? lastBytes;
      onProgress(progress);
    });
    child.once("error", () => {
      cleanup();
      reject(new Error("decision_model_install_process_failed"));
    });
    child.once("exit", (code) => {
      cleanup();
      if (code === 0 && completed && !invalid) resolve();
      else
        reject(
          new Error(
            signal.aborted ? "decision_model_install_cancelled" : "decision_model_install_failed",
          ),
        );
    });
    signal.addEventListener("abort", cancel, { once: true });
    if (signal.aborted) cancel();
  });
}

export function parseDecisionInstallProgress(value: unknown): DecisionInstallProgress | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  if (record.type !== "progress") return null;
  const phase = record.phase;
  if (
    phase !== "checking" &&
    phase !== "waiting-for-lock" &&
    phase !== "downloading" &&
    phase !== "verifying" &&
    phase !== "installing"
  )
    return null;
  const bytes = record.downloadedBytes;
  const total = record.totalBytes;
  if (bytes === undefined && total === undefined) return { phase };
  if (
    typeof bytes !== "number" ||
    !Number.isSafeInteger(bytes) ||
    bytes < 0 ||
    bytes > DECISION_MODEL_TOTAL_BYTES ||
    total !== DECISION_MODEL_TOTAL_BYTES
  )
    return null;
  return { phase, downloadedBytes: bytes, totalBytes: DECISION_MODEL_TOTAL_BYTES };
}

function isCompleted(value: unknown): boolean {
  return (
    typeof value === "object" && value !== null && "type" in value && value.type === "completed"
  );
}
