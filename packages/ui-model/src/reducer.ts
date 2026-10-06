import type {
  ApprovalState,
  UiModelAction,
  UiModelState,
} from "@lyonbrown/ui-model/internal/types";
import { getFreshCurrentServerInstanceId } from "@lyonbrown/ui-model/internal/epoch";

function failClosedApproval(
  approval: ApprovalState | null,
  status: "disconnected" | "errored" | "failed" | "timed-out",
): ApprovalState | null {
  return approval === null ? null : { ...approval, status };
}

function acceptSnapshot(
  state: UiModelState,
  action: UiModelAction & { type: "snapshotReceived" },
): UiModelState {
  const { snapshot } = action;
  if (state.remote.connection !== "synchronizing" && state.remote.connection !== "ready") {
    return state;
  }
  if (snapshot.serverInstanceId !== state.remote.serverInstanceId) {
    return state;
  }

  const previous = state.remote.snapshot;
  if (previous?.serverInstanceId === snapshot.serverInstanceId) {
    if (snapshot.revision < previous.revision) {
      return state;
    }
    if (snapshot.revision === previous.revision) {
      if (state.remote.connection === "ready") {
        return state;
      }
      return {
        ...state,
        remote: {
          ...state.remote,
          connection: "ready",
          snapshotFresh: true,
          unavailableReason: null,
        },
      };
    }
  }

  const sessionChanged = previous !== null && previous.sessionId !== snapshot.sessionId;
  const epochChanged = previous !== null && previous.serverInstanceId !== snapshot.serverInstanceId;

  return {
    ...state,
    remote: {
      connection: "ready" as const,
      serverInstanceId: state.remote.serverInstanceId,
      snapshot,
      snapshotFresh: true,
      unavailableReason: null,
    },
    inspector:
      sessionChanged || epochChanged ? { ...state.inspector, selection: null } : state.inspector,
  };
}

export function reduceUiModel(state: UiModelState, action: UiModelAction): UiModelState {
  switch (action.type) {
    case "clientOpened":
      return state;
    case "connectRequested":
      return {
        ...state,
        remote: {
          ...state.remote,
          connection: "connecting",
          snapshotFresh: false,
          unavailableReason: null,
        },
      };
    case "clientDisconnected":
      return {
        ...state,
        remote: {
          ...state.remote,
          connection: state.remote.snapshot === null ? "disconnected" : "stale",
          snapshotFresh: false,
        },
        approval: failClosedApproval(state.approval, "disconnected"),
      };
    case "connectionLost":
      return {
        ...state,
        remote: {
          ...state.remote,
          connection: state.remote.snapshot === null ? "disconnected" : "stale",
          snapshotFresh: false,
          unavailableReason: action.reason,
        },
        approval: failClosedApproval(state.approval, "disconnected"),
      };
    case "connectionUnavailable":
      return {
        ...state,
        remote: {
          ...state.remote,
          connection: "unavailable",
          snapshotFresh: false,
          unavailableReason: action.reason,
        },
        approval: failClosedApproval(state.approval, "disconnected"),
      };
    case "handshakeSucceeded": {
      const epochChanged = state.remote.serverInstanceId !== action.serverInstanceId;
      return {
        ...state,
        remote: {
          ...state.remote,
          connection: "synchronizing",
          serverInstanceId: action.serverInstanceId,
          snapshotFresh: false,
          unavailableReason: null,
        },
        modelDownloadIntent: epochChanged ? null : state.modelDownloadIntent,
        approval: epochChanged ? null : state.approval,
        inspector: epochChanged ? { ...state.inspector, selection: null } : state.inspector,
      };
    }
    case "snapshotReceived":
      return acceptSnapshot(state, action);
    case "modelDownloadRequested": {
      const serverInstanceId = getFreshCurrentServerInstanceId(state);
      if (serverInstanceId === null) {
        return state;
      }
      return {
        ...state,
        modelDownloadIntent: {
          operationId: action.operationId,
          modelId: action.modelId,
          serverInstanceId,
        },
      };
    }
    case "modelDownloadIntentAcknowledged":
      return state.modelDownloadIntent?.operationId === action.operationId &&
        state.modelDownloadIntent.serverInstanceId === action.serverInstanceId &&
        state.remote.serverInstanceId === action.serverInstanceId
        ? { ...state, modelDownloadIntent: null }
        : state;
    case "approvalRequested": {
      const serverInstanceId = getFreshCurrentServerInstanceId(state);
      return serverInstanceId === null
        ? state
        : {
            ...state,
            approval: { request: action.request, serverInstanceId, status: "pending" },
          };
    }
    case "approvalDenied":
      return state.approval === null
        ? state
        : { ...state, approval: { ...state.approval, status: "denying" } };
    case "approvalTimedOut":
      return { ...state, approval: failClosedApproval(state.approval, "timed-out") };
    case "approvalErrored":
      return { ...state, approval: failClosedApproval(state.approval, "errored") };
    case "approvalFailed":
      return { ...state, approval: failClosedApproval(state.approval, "failed") };
    case "approvalResolved":
      return state.approval?.request.requestId === action.requestId &&
        state.approval.serverInstanceId === action.serverInstanceId &&
        state.remote.serverInstanceId === action.serverInstanceId
        ? { ...state, approval: null }
        : state;
    case "activitySelected":
      return { ...state, inspector: { ...state.inspector, selection: action.selection } };
    case "followLatestChanged":
      return { ...state, inspector: { ...state.inspector, followLatest: action.value } };
    case "narrowViewModeChanged":
      return { ...state, inspector: { ...state.inspector, narrowViewMode: action.mode } };
    case "localeChanged":
      return { ...state, preferences: { ...state.preferences, locale: action.locale } };
    case "themeChanged":
      return { ...state, preferences: { ...state.preferences, themeId: action.themeId } };
  }
}
