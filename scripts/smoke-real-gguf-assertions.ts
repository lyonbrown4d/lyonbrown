import type { SmokeSnapshot } from "@lyonbrown/workspace/internal/smoke-real-gguf-flow";

const PUBLIC_FAILURE_CODE = /^[A-Za-z0-9_-]{1,64}$/u;

export function assertSmokeSnapshotHealthy(snapshot: SmokeSnapshot): void {
  const failure = snapshot.modelSetup?.failure;
  if (snapshot.modelSetup?.phase === "failed" || (failure !== null && failure !== undefined)) {
    const code =
      failure !== null && failure !== undefined && PUBLIC_FAILURE_CODE.test(failure.code)
        ? failure.code
        : "unknown";
    throw new Error(`REAL_GGUF_MODEL_SETUP_FAILED:${code}`);
  }
  if (snapshot.phase === "failed-recoverable" || snapshot.phase === "failed-terminal") {
    throw new Error("REAL_GGUF_RUNTIME_FAILED");
  }
}

export function isSetup(snapshot: SmokeSnapshot, phase: string): boolean {
  return snapshot.modelSetup?.phase === phase;
}

export function hasProgress(snapshot: SmokeSnapshot, minimumBytes: bigint): boolean {
  return (
    snapshot.modelSetup?.phase === "downloading" &&
    snapshot.modelSetup.allowedActions.includes("cancel") &&
    snapshot.modelSetup.downloadedBytes >= minimumBytes
  );
}

export function isReady(snapshot: SmokeSnapshot): boolean {
  const setup = snapshot.modelSetup;
  return (
    snapshot.phase === "ready" &&
    setup?.phase === "ready" &&
    setup.operationId === null &&
    setup.failure === null
  );
}

export function isActiveDownload(setup: NonNullable<SmokeSnapshot["modelSetup"]>): boolean {
  return (
    ["waiting-for-lock", "downloading", "verifying"].includes(setup.phase) &&
    setup.allowedActions.includes("cancel")
  );
}

export function requireManagedModel(
  snapshot: SmokeSnapshot,
): NonNullable<NonNullable<SmokeSnapshot["modelSetup"]>["managedModel"]> {
  const model = snapshot.modelSetup?.managedModel;
  if (model === null || model === undefined) throw new Error("REAL_GGUF_MODEL_METADATA_MISSING");
  return model;
}

export function requireOperationId(snapshot: SmokeSnapshot): string {
  const operationId = snapshot.modelSetup?.operationId;
  if (operationId === null || operationId === undefined || operationId.length === 0) {
    throw new Error("REAL_GGUF_OPERATION_ID_MISSING");
  }
  return operationId;
}

export function requireSessionId(snapshot: SmokeSnapshot): string {
  if (snapshot.sessionId === null || snapshot.sessionId.length === 0) {
    throw new Error("REAL_GGUF_SESSION_ID_MISSING");
  }
  return snapshot.sessionId;
}
