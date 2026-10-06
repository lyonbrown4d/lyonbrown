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
import { describe, expect, it } from "vitest";

import {
  activityPhase,
  approvalState,
  disabledReason,
  modelAction,
  modelLocality,
  modelRecommendationReason,
  modelSetupPhase,
  modelSource,
  modelTier,
  risk,
  runtimePhase,
  sandbox,
  stopReason,
  toolState,
  turnStatus,
} from "@lyonbrown/agent-client/internal/snapshot-scalars";

describe("snapshot semantic enum mapping", () => {
  it.each([
    [RuntimePhase.RUNTIME_PHASE_INITIALIZING, "initializing"],
    [RuntimePhase.RUNTIME_PHASE_MODEL_SETUP_REQUIRED, "model-setup-required"],
    [RuntimePhase.RUNTIME_PHASE_READY, "ready"],
    [RuntimePhase.RUNTIME_PHASE_PREPARING, "preparing"],
    [RuntimePhase.RUNTIME_PHASE_STREAMING, "streaming"],
    [RuntimePhase.RUNTIME_PHASE_AWAITING_APPROVAL, "awaiting-approval"],
    [RuntimePhase.RUNTIME_PHASE_RUNNING_TOOL, "running-tool"],
    [RuntimePhase.RUNTIME_PHASE_CANCELLING, "cancelling"],
    [RuntimePhase.RUNTIME_PHASE_COMPLETED, "completed"],
    [RuntimePhase.RUNTIME_PHASE_FAILED_RECOVERABLE, "failed-recoverable"],
    [RuntimePhase.RUNTIME_PHASE_FAILED_TERMINAL, "failed-terminal"],
  ])("maps runtime phase %s", (wire, dto) => {
    expect(runtimePhase(wire)).toBe(dto);
  });

  it.each([
    [TurnStatus.TURN_STATUS_PENDING, "pending"],
    [TurnStatus.TURN_STATUS_RUNNING, "running"],
    [TurnStatus.TURN_STATUS_COMPLETED, "completed"],
    [TurnStatus.TURN_STATUS_CANCELLED, "cancelled"],
    [TurnStatus.TURN_STATUS_FAILED, "failed"],
  ])("maps turn status %s", (wire, dto) => {
    expect(turnStatus(wire)).toBe(dto);
  });

  it.each([
    [ActivityPhase.ACTIVITY_PHASE_PREPARING, "preparing"],
    [ActivityPhase.ACTIVITY_PHASE_STREAMING, "streaming"],
    [ActivityPhase.ACTIVITY_PHASE_AWAITING_APPROVAL, "awaiting-approval"],
    [ActivityPhase.ACTIVITY_PHASE_RUNNING_TOOL, "running-tool"],
    [ActivityPhase.ACTIVITY_PHASE_SUPERSEDED, "superseded"],
    [ActivityPhase.ACTIVITY_PHASE_COMPLETED, "completed"],
    [ActivityPhase.ACTIVITY_PHASE_FAILED, "failed"],
  ])("maps activity phase %s", (wire, dto) => {
    expect(activityPhase(wire)).toBe(dto);
  });

  it.each([
    [ToolActivityState.TOOL_ACTIVITY_STATE_REQUESTED, "requested"],
    [ToolActivityState.TOOL_ACTIVITY_STATE_APPROVED, "approved"],
    [ToolActivityState.TOOL_ACTIVITY_STATE_RUNNING, "running"],
    [ToolActivityState.TOOL_ACTIVITY_STATE_COMPLETED, "completed"],
    [ToolActivityState.TOOL_ACTIVITY_STATE_FAILED, "failed"],
  ])("maps tool state %s", (wire, dto) => {
    expect(toolState(wire)).toBe(dto);
  });

  it.each([
    [StopReasonKind.STOP_REASON_KIND_END_TURN, "end-turn"],
    [StopReasonKind.STOP_REASON_KIND_TOOL_USE, "tool-use"],
    [StopReasonKind.STOP_REASON_KIND_MAX_TOKENS, "max-tokens"],
    [StopReasonKind.STOP_REASON_KIND_STOP, "stop"],
    [StopReasonKind.STOP_REASON_KIND_ERROR, "error"],
  ])("maps stop reason %s", (wire, dto) => {
    expect(stopReason(wire)).toBe(dto);
  });

  it.each([
    [ApprovalState.APPROVAL_STATE_PENDING, "pending"],
    [ApprovalState.APPROVAL_STATE_APPROVED, "approved"],
  ])("maps approval state %s", (wire, dto) => {
    expect(approvalState(wire)).toBe(dto);
  });

  it.each([
    [RiskLevel.RISK_LEVEL_READ, "read"],
    [RiskLevel.RISK_LEVEL_WRITE, "write"],
    [RiskLevel.RISK_LEVEL_EXECUTE, "execute"],
    [RiskLevel.RISK_LEVEL_NETWORK, "network"],
  ])("maps risk %s", (wire, dto) => {
    expect(risk(wire)).toBe(dto);
  });

  it.each([
    [SandboxStatus.SANDBOX_STATUS_WORKSPACE_RESTRICTED, "workspace-restricted"],
    [SandboxStatus.SANDBOX_STATUS_HOST_UNSANDBOXED, "host-unsandboxed"],
    [SandboxStatus.SANDBOX_STATUS_UNKNOWN, "unknown"],
  ])("maps sandbox %s", (wire, dto) => {
    expect(sandbox(wire)).toBe(dto);
  });

  it.each([
    [ModelSetupSource.MODEL_SETUP_SOURCE_MANAGED_PRESET, "managed-preset"],
    [ModelSetupSource.MODEL_SETUP_SOURCE_CONFIGURED_PATH, "configured-path"],
  ])("maps model source %s", (wire, dto) => {
    expect(modelSource(wire)).toBe(dto);
  });

  it.each([
    [ModelSetupPhase.MODEL_SETUP_PHASE_REQUIRED, "required"],
    [ModelSetupPhase.MODEL_SETUP_PHASE_WAITING_FOR_LOCK, "waiting-for-lock"],
    [ModelSetupPhase.MODEL_SETUP_PHASE_CHECKING, "checking"],
    [ModelSetupPhase.MODEL_SETUP_PHASE_DOWNLOADING, "downloading"],
    [ModelSetupPhase.MODEL_SETUP_PHASE_VERIFYING, "verifying"],
    [ModelSetupPhase.MODEL_SETUP_PHASE_INSTALLING, "installing"],
    [ModelSetupPhase.MODEL_SETUP_PHASE_ACTIVATING, "activating"],
    [ModelSetupPhase.MODEL_SETUP_PHASE_READY, "ready"],
    [ModelSetupPhase.MODEL_SETUP_PHASE_CANCELLED, "cancelled"],
    [ModelSetupPhase.MODEL_SETUP_PHASE_FAILED, "failed"],
  ])("maps model setup phase %s", (wire, dto) => {
    expect(modelSetupPhase(wire)).toBe(dto);
  });

  it.each([
    [ModelSetupAction.MODEL_SETUP_ACTION_DOWNLOAD, "download"],
    [ModelSetupAction.MODEL_SETUP_ACTION_CANCEL, "cancel"],
    [ModelSetupAction.MODEL_SETUP_ACTION_RETRY, "retry"],
  ])("maps model action %s", (wire, dto) => {
    expect(modelAction(wire)).toBe(dto);
  });

  it.each([
    [undefined, null],
    [
      ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_INSTALLED_PREFERRED,
      "installed-preferred",
    ],
    [
      ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_HARDWARE_COMPATIBLE,
      "hardware-compatible",
    ],
    [
      ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_CONSERVATIVE_FALLBACK,
      "conservative-fallback",
    ],
  ])("maps model recommendation reason %s", (wire, dto) => {
    expect(modelRecommendationReason(wire)).toBe(dto);
  });

  it.each([
    [ModelTier.MODEL_TIER_CONTROL, "control"],
    [ModelTier.MODEL_TIER_WORKER, "worker"],
    [ModelTier.MODEL_TIER_FRONTIER, "frontier"],
  ])("maps model tier %s", (wire, dto) => {
    expect(modelTier(wire)).toBe(dto);
  });

  it.each([
    [ModelLocality.MODEL_LOCALITY_LOCAL, "local"],
    [ModelLocality.MODEL_LOCALITY_REMOTE, "remote"],
  ])("maps model locality %s", (wire, dto) => {
    expect(modelLocality(wire)).toBe(dto);
  });

  it.each([
    [undefined, null],
    [ModelOptionDisabledReason.MODEL_OPTION_DISABLED_REASON_NOT_WORKER_TIER, "not-worker-tier"],
    [ModelOptionDisabledReason.MODEL_OPTION_DISABLED_REASON_TOOLS_UNSUPPORTED, "tools-unsupported"],
    [
      ModelOptionDisabledReason.MODEL_OPTION_DISABLED_REASON_STREAMING_UNSUPPORTED,
      "streaming-unsupported",
    ],
  ])("maps disabled reason %s", (wire, dto) => {
    expect(disabledReason(wire)).toBe(dto);
  });

  it.each([
    [runtimePhase, RuntimePhase.RUNTIME_PHASE_UNSPECIFIED],
    [turnStatus, TurnStatus.TURN_STATUS_UNSPECIFIED],
    [activityPhase, ActivityPhase.ACTIVITY_PHASE_UNSPECIFIED],
    [toolState, ToolActivityState.TOOL_ACTIVITY_STATE_UNSPECIFIED],
    [stopReason, StopReasonKind.STOP_REASON_KIND_UNSPECIFIED],
    [approvalState, ApprovalState.APPROVAL_STATE_UNSPECIFIED],
    [risk, RiskLevel.RISK_LEVEL_UNSPECIFIED],
    [sandbox, SandboxStatus.SANDBOX_STATUS_UNSPECIFIED],
    [modelSource, ModelSetupSource.MODEL_SETUP_SOURCE_UNSPECIFIED],
    [modelSetupPhase, ModelSetupPhase.MODEL_SETUP_PHASE_UNSPECIFIED],
    [modelAction, ModelSetupAction.MODEL_SETUP_ACTION_UNSPECIFIED],
    [modelRecommendationReason, ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_UNSPECIFIED],
    [modelTier, ModelTier.MODEL_TIER_UNSPECIFIED],
    [modelLocality, ModelLocality.MODEL_LOCALITY_UNSPECIFIED],
    [disabledReason, ModelOptionDisabledReason.MODEL_OPTION_DISABLED_REASON_UNSPECIFIED],
  ])("rejects unspecified enum through mapper %#", (mapper, value) => {
    expect(() => mapper(value as never)).toThrow(/unknown enum/u);
  });
});
