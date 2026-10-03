import type {
  ApprovalDisposition,
  InputPriority,
  ModelSetupCancellation,
  MutationAvailability,
  UiModelState,
} from "./types.js";

export function getMutationAvailability(state: UiModelState): MutationAvailability {
  switch (state.remote.connection) {
    case "disconnected":
      return { enabled: false, reason: "disconnected" };
    case "connecting":
      return { enabled: false, reason: "connecting" };
    case "synchronizing":
      return { enabled: false, reason: "synchronizing" };
    case "stale":
      return { enabled: false, reason: "stale-snapshot" };
    case "unavailable":
      return { enabled: false, reason: "unavailable" };
    case "ready":
      break;
  }

  if (!state.remote.snapshotFresh) {
    return { enabled: false, reason: "stale-snapshot" };
  }
  if (
    state.remote.snapshot === null ||
    state.remote.snapshot.serverInstanceId !== state.remote.serverInstanceId
  ) {
    return { enabled: false, reason: "wrong-epoch" };
  }

  const modelSetup = state.remote.snapshot.modelSetup;
  if (modelSetup !== null && modelSetup.phase !== "Ready") {
    return { enabled: false, reason: "model-setup-required" };
  }
  return { enabled: true };
}

export const getDaemonMutationAvailability = getMutationAvailability;
export const getChatSubmitAvailability = getMutationAvailability;

export function getModelSetupCancellation(state: UiModelState): ModelSetupCancellation {
  const modelSetup = state.remote.snapshot?.modelSetup;
  if (modelSetup === null || modelSetup === undefined) {
    return { allowed: false, reason: "no-operation" };
  }

  switch (modelSetup.phase) {
    case "WaitingForLock":
    case "Checking":
    case "Downloading":
    case "Verifying":
      return { allowed: true, operationId: modelSetup.operationId };
    case "Installing":
    case "Activating":
      return { allowed: false, reason: "too-late" };
    case "Ready":
    case "Cancelled":
    case "Failed":
      return { allowed: false, reason: "terminal" };
  }
}

export function getApprovalDisposition(state: UiModelState): ApprovalDisposition {
  if (state.approval === null) {
    return { kind: "none" };
  }

  const requestId = state.approval.request.requestId;
  switch (state.approval.status) {
    case "pending":
      return { kind: "decision-required", requestId };
    case "denying":
      return { kind: "deny-intent", requestId, reason: "user" };
    case "timed-out":
      return { kind: "deny-intent", requestId, reason: "timeout" };
    case "errored":
      return { kind: "deny-intent", requestId, reason: "error" };
    case "failed":
      return { kind: "deny-intent", requestId, reason: "failure" };
    case "disconnected":
      return { kind: "deny-intent", requestId, reason: "disconnected" };
  }
}

export function getInputPriority(state: UiModelState): InputPriority {
  if (state.approval !== null) {
    return "approval";
  }
  const modelSetup = state.remote.snapshot?.modelSetup;
  return modelSetup !== null && modelSetup !== undefined && modelSetup.phase !== "Ready"
    ? "model-setup"
    : "composer";
}
