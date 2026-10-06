import {
  ActivityPhase,
  ApprovalState,
  ModelLocality,
  ModelOptionDisabledReason,
  ModelRecommendationReason,
  ModelSetupAction,
  ModelSetupPhase,
  ModelSetupSource,
  ModelTier,
  RiskLevel,
  RuntimePhase,
  SandboxStatus,
  StopReasonKind,
  ToolActivityState,
  TurnStatus,
} from "@lyonbrown/protocol";

import { clientError } from "./errors.ts";
import type { RuntimePhaseDto } from "./types.ts";

function unknown(field: string): never {
  throw clientError("protocol", "INVALID_SNAPSHOT", `${field} contains an unknown enum value`);
}

export function runtimePhase(value: RuntimePhase): RuntimePhaseDto {
  switch (value) {
    case RuntimePhase.RUNTIME_PHASE_INITIALIZING:
      return "initializing";
    case RuntimePhase.RUNTIME_PHASE_MODEL_SETUP_REQUIRED:
      return "model-setup-required";
    case RuntimePhase.RUNTIME_PHASE_READY:
      return "ready";
    case RuntimePhase.RUNTIME_PHASE_PREPARING:
      return "preparing";
    case RuntimePhase.RUNTIME_PHASE_STREAMING:
      return "streaming";
    case RuntimePhase.RUNTIME_PHASE_AWAITING_APPROVAL:
      return "awaiting-approval";
    case RuntimePhase.RUNTIME_PHASE_RUNNING_TOOL:
      return "running-tool";
    case RuntimePhase.RUNTIME_PHASE_CANCELLING:
      return "cancelling";
    case RuntimePhase.RUNTIME_PHASE_COMPLETED:
      return "completed";
    case RuntimePhase.RUNTIME_PHASE_FAILED_RECOVERABLE:
      return "failed-recoverable";
    case RuntimePhase.RUNTIME_PHASE_FAILED_TERMINAL:
      return "failed-terminal";
    default:
      return unknown("runtime.phase");
  }
}

export function turnStatus(value: TurnStatus) {
  switch (value) {
    case TurnStatus.TURN_STATUS_PENDING:
      return "pending" as const;
    case TurnStatus.TURN_STATUS_RUNNING:
      return "running" as const;
    case TurnStatus.TURN_STATUS_COMPLETED:
      return "completed" as const;
    case TurnStatus.TURN_STATUS_CANCELLED:
      return "cancelled" as const;
    case TurnStatus.TURN_STATUS_FAILED:
      return "failed" as const;
    default:
      return unknown("turn.status");
  }
}

export function activityPhase(value: ActivityPhase) {
  switch (value) {
    case ActivityPhase.ACTIVITY_PHASE_PREPARING:
      return "preparing" as const;
    case ActivityPhase.ACTIVITY_PHASE_STREAMING:
      return "streaming" as const;
    case ActivityPhase.ACTIVITY_PHASE_AWAITING_APPROVAL:
      return "awaiting-approval" as const;
    case ActivityPhase.ACTIVITY_PHASE_RUNNING_TOOL:
      return "running-tool" as const;
    case ActivityPhase.ACTIVITY_PHASE_SUPERSEDED:
      return "superseded" as const;
    case ActivityPhase.ACTIVITY_PHASE_COMPLETED:
      return "completed" as const;
    case ActivityPhase.ACTIVITY_PHASE_FAILED:
      return "failed" as const;
    default:
      return unknown("activity.phase");
  }
}

export function toolState(value: ToolActivityState) {
  switch (value) {
    case ToolActivityState.TOOL_ACTIVITY_STATE_REQUESTED:
      return "requested" as const;
    case ToolActivityState.TOOL_ACTIVITY_STATE_APPROVED:
      return "approved" as const;
    case ToolActivityState.TOOL_ACTIVITY_STATE_RUNNING:
      return "running" as const;
    case ToolActivityState.TOOL_ACTIVITY_STATE_COMPLETED:
      return "completed" as const;
    case ToolActivityState.TOOL_ACTIVITY_STATE_FAILED:
      return "failed" as const;
    default:
      return unknown("tool.state");
  }
}

export function stopReason(value: StopReasonKind) {
  switch (value) {
    case StopReasonKind.STOP_REASON_KIND_END_TURN:
      return "end-turn" as const;
    case StopReasonKind.STOP_REASON_KIND_TOOL_USE:
      return "tool-use" as const;
    case StopReasonKind.STOP_REASON_KIND_MAX_TOKENS:
      return "max-tokens" as const;
    case StopReasonKind.STOP_REASON_KIND_STOP:
      return "stop" as const;
    case StopReasonKind.STOP_REASON_KIND_ERROR:
      return "error" as const;
    default:
      return unknown("stop_reason.kind");
  }
}

export function approvalState(value: ApprovalState) {
  switch (value) {
    case ApprovalState.APPROVAL_STATE_PENDING:
      return "pending" as const;
    case ApprovalState.APPROVAL_STATE_APPROVED:
      return "approved" as const;
    default:
      return unknown("approval.state");
  }
}

export function risk(value: RiskLevel) {
  switch (value) {
    case RiskLevel.RISK_LEVEL_READ:
      return "read" as const;
    case RiskLevel.RISK_LEVEL_WRITE:
      return "write" as const;
    case RiskLevel.RISK_LEVEL_EXECUTE:
      return "execute" as const;
    case RiskLevel.RISK_LEVEL_NETWORK:
      return "network" as const;
    default:
      return unknown("preview.risk");
  }
}

export function sandbox(value: SandboxStatus) {
  switch (value) {
    case SandboxStatus.SANDBOX_STATUS_WORKSPACE_RESTRICTED:
      return "workspace-restricted" as const;
    case SandboxStatus.SANDBOX_STATUS_HOST_UNSANDBOXED:
      return "host-unsandboxed" as const;
    case SandboxStatus.SANDBOX_STATUS_UNKNOWN:
      return "unknown" as const;
    default:
      return unknown("preview.sandbox");
  }
}

export function modelSource(value: ModelSetupSource) {
  switch (value) {
    case ModelSetupSource.MODEL_SETUP_SOURCE_MANAGED_PRESET:
      return "managed-preset" as const;
    case ModelSetupSource.MODEL_SETUP_SOURCE_CONFIGURED_PATH:
      return "configured-path" as const;
    default:
      return unknown("model_setup.source");
  }
}

export function modelSetupPhase(value: ModelSetupPhase) {
  switch (value) {
    case ModelSetupPhase.MODEL_SETUP_PHASE_REQUIRED:
      return "required" as const;
    case ModelSetupPhase.MODEL_SETUP_PHASE_WAITING_FOR_LOCK:
      return "waiting-for-lock" as const;
    case ModelSetupPhase.MODEL_SETUP_PHASE_CHECKING:
      return "checking" as const;
    case ModelSetupPhase.MODEL_SETUP_PHASE_DOWNLOADING:
      return "downloading" as const;
    case ModelSetupPhase.MODEL_SETUP_PHASE_VERIFYING:
      return "verifying" as const;
    case ModelSetupPhase.MODEL_SETUP_PHASE_INSTALLING:
      return "installing" as const;
    case ModelSetupPhase.MODEL_SETUP_PHASE_ACTIVATING:
      return "activating" as const;
    case ModelSetupPhase.MODEL_SETUP_PHASE_READY:
      return "ready" as const;
    case ModelSetupPhase.MODEL_SETUP_PHASE_CANCELLED:
      return "cancelled" as const;
    case ModelSetupPhase.MODEL_SETUP_PHASE_FAILED:
      return "failed" as const;
    default:
      return unknown("model_setup.phase");
  }
}

export function modelAction(value: ModelSetupAction) {
  switch (value) {
    case ModelSetupAction.MODEL_SETUP_ACTION_DOWNLOAD:
      return "download" as const;
    case ModelSetupAction.MODEL_SETUP_ACTION_CANCEL:
      return "cancel" as const;
    case ModelSetupAction.MODEL_SETUP_ACTION_RETRY:
      return "retry" as const;
    default:
      return unknown("model_setup.allowed_actions");
  }
}

export function modelRecommendationReason(value: ModelRecommendationReason | undefined) {
  switch (value) {
    case undefined:
      return null;
    case ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_INSTALLED_PREFERRED:
      return "installed-preferred" as const;
    case ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_HARDWARE_COMPATIBLE:
      return "hardware-compatible" as const;
    case ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_CONSERVATIVE_FALLBACK:
      return "conservative-fallback" as const;
    default:
      return unknown("model_setup.managed_model.recommendation_reason");
  }
}

export function modelTier(value: ModelTier) {
  switch (value) {
    case ModelTier.MODEL_TIER_CONTROL:
      return "control" as const;
    case ModelTier.MODEL_TIER_WORKER:
      return "worker" as const;
    case ModelTier.MODEL_TIER_FRONTIER:
      return "frontier" as const;
    default:
      return unknown("model.tier");
  }
}

export function modelLocality(value: ModelLocality) {
  switch (value) {
    case ModelLocality.MODEL_LOCALITY_LOCAL:
      return "local" as const;
    case ModelLocality.MODEL_LOCALITY_REMOTE:
      return "remote" as const;
    default:
      return unknown("model.locality");
  }
}

export function disabledReason(value: ModelOptionDisabledReason | undefined) {
  switch (value) {
    case undefined:
      return null;
    case ModelOptionDisabledReason.MODEL_OPTION_DISABLED_REASON_NOT_WORKER_TIER:
      return "not-worker-tier" as const;
    case ModelOptionDisabledReason.MODEL_OPTION_DISABLED_REASON_TOOLS_UNSUPPORTED:
      return "tools-unsupported" as const;
    case ModelOptionDisabledReason.MODEL_OPTION_DISABLED_REASON_STREAMING_UNSUPPORTED:
      return "streaming-unsupported" as const;
    default:
      return unknown("model.disabled_reason");
  }
}
