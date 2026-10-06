import type {
  ApprovalDenyIntent,
  ApprovalDisposition,
  ChatSubmitAvailability,
  ConnectionFreshness,
  InputPriority,
  ModelDownloadIntent,
  ModelSetupCancellation,
  UiModelState,
} from "./types.ts";
import { getFreshCurrentServerInstanceId } from "./epoch.ts";

export function getConnectionFreshness(state: UiModelState): ConnectionFreshness {
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
  return { enabled: true };
}

export function getDaemonMutationAvailability(state: UiModelState): ConnectionFreshness {
  return getConnectionFreshness(state);
}

export function getChatSubmitAvailability(state: UiModelState): ChatSubmitAvailability {
  const freshness = getConnectionFreshness(state);
  if (!freshness.enabled) {
    return freshness;
  }

  const modelSetup = state.remote.snapshot?.modelSetup;
  return modelSetup !== null && modelSetup !== undefined && modelSetup.phase !== "Ready"
    ? { enabled: false, reason: "model-setup-required" }
    : { enabled: true };
}

export function getModelSetupCancellation(state: UiModelState): ModelSetupCancellation {
  const modelSetup = state.remote.snapshot?.modelSetup;
  if (
    modelSetup === null ||
    modelSetup === undefined ||
    modelSetup.phase === "Required" ||
    modelSetup.phase === "Ready"
  ) {
    return { allowed: false, reason: "no-operation" };
  }

  switch (modelSetup.phase) {
    case "WaitingForLock":
    case "Checking":
    case "Downloading":
    case "Verifying":
      return modelSetup.operationId === null
        ? { allowed: false, reason: "no-operation" }
        : { allowed: true, operationId: modelSetup.operationId };
    case "Installing":
    case "Activating":
      return { allowed: false, reason: "too-late" };
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
  const { serverInstanceId } = state.approval;
  switch (state.approval.status) {
    case "pending":
      return { kind: "decision-required", requestId, serverInstanceId };
    case "denying":
      return { kind: "deny-intent", requestId, reason: "user", serverInstanceId };
    case "timed-out":
      return { kind: "deny-intent", requestId, reason: "timeout", serverInstanceId };
    case "errored":
      return { kind: "deny-intent", requestId, reason: "error", serverInstanceId };
    case "failed":
      return { kind: "deny-intent", requestId, reason: "failure", serverInstanceId };
    case "disconnected":
      return { kind: "deny-intent", requestId, reason: "disconnected", serverInstanceId };
  }
}

export function getSendableApprovalDenyIntent(state: UiModelState): ApprovalDenyIntent | null {
  const disposition = getApprovalDisposition(state);
  const currentServerInstanceId = getFreshCurrentServerInstanceId(state);
  if (
    disposition.kind !== "deny-intent" ||
    currentServerInstanceId === null ||
    disposition.serverInstanceId !== currentServerInstanceId
  ) {
    return null;
  }
  return {
    requestId: disposition.requestId,
    reason: disposition.reason,
    serverInstanceId: disposition.serverInstanceId,
  };
}

export function getSendableModelDownloadIntent(state: UiModelState): ModelDownloadIntent | null {
  const intent = state.modelDownloadIntent;
  const currentServerInstanceId = getFreshCurrentServerInstanceId(state);
  return intent !== null && intent.serverInstanceId === currentServerInstanceId ? intent : null;
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
