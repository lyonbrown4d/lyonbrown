import type {
  ActivitySnapshot,
  ApprovalSnapshot,
  StepOutcome,
  ToolActivity,
  ToolPreview,
  TurnSnapshot,
} from "@lyonbrown/protocol";

import {
  activityPhase,
  approvalState,
  risk,
  sandbox,
  stopReason,
  toolState,
  turnStatus,
} from "./snapshot-scalars.ts";
import { mapRouteSummary } from "./snapshot-model.ts";
import type {
  ActivitySnapshotDto,
  ApprovalSnapshotDto,
  StepOutcomeDto,
  ToolActivityDto,
  ToolPreviewDto,
  TurnSnapshotDto,
} from "./types.ts";
import { protocolId, protocolText, requireJson, requiredField } from "./validation.ts";

export function mapTurn(value: TurnSnapshot): TurnSnapshotDto {
  return {
    turnId: protocolId("turn.turn_id", value.turnId),
    userText: protocolText("turn.user_text", value.userText),
    assistantText: protocolText("turn.assistant_text", value.assistantText),
    status: turnStatus(value.status),
    steering: value.steering.map((text) => protocolText("turn.steering", text)),
    activities: value.activities.map(mapActivity),
  };
}

function mapActivity(value: ActivitySnapshot): ActivitySnapshotDto {
  return {
    stepId: protocolId("activity.step_id", value.stepId),
    phase: activityPhase(value.phase),
    route: mapRouteSummary(value.route),
    context:
      value.context === undefined
        ? null
        : {
            systemTokens: value.context.systemTokens,
            contextTokens: value.context.contextTokens,
            dropped: value.context.dropped,
          },
    assistantText: protocolText("activity.assistant_text", value.assistantText),
    usage:
      value.usage === undefined
        ? null
        : {
            input: value.usage.input,
            output: value.usage.output,
            cacheRead: value.usage.cacheRead,
          },
    tools: value.tools.map(mapTool),
    outcome: value.outcome === undefined ? null : mapOutcome(value.outcome),
  };
}

function mapTool(value: ToolActivity): ToolActivityDto {
  return {
    callId: protocolId("tool.call_id", value.callId),
    name: protocolId("tool.name", value.name),
    argumentsJson: requireJson("tool.arguments_json", value.argumentsJson),
    state: toolState(value.state),
    approvedBy:
      value.approvedBy === undefined ? null : protocolId("tool.approved_by", value.approvedBy),
    result:
      value.result === undefined
        ? null
        : {
            invocationId: protocolId("tool.result.invocation_id", value.result.invocationId),
            ok: value.result.ok,
            output: protocolText("tool.result.output", value.result.output),
            truncated: value.result.truncated,
            durationMs: value.result.durationMs,
            error:
              value.result.error === undefined
                ? null
                : protocolText("tool.result.error", value.result.error),
          },
  };
}

function mapOutcome(value: StepOutcome): StepOutcomeDto {
  const reason = requiredField("outcome.stop_reason", value.stopReason);
  return {
    stopReason: {
      kind: stopReason(reason.kind),
      error: reason.error === undefined ? null : protocolText("stop_reason.error", reason.error),
    },
    text: protocolText("outcome.text", value.text),
    toolCallIds: value.toolCallIds.map((id) => protocolId("outcome.tool_call_id", id)),
    escalated: value.escalated,
  };
}

export function mapApproval(value: ApprovalSnapshot | undefined): ApprovalSnapshotDto | null {
  if (value === undefined) return null;
  return {
    requestId: protocolId("approval.request_id", value.requestId),
    toolName: protocolId("approval.tool_name", value.toolName),
    argumentsJson: requireJson("approval.arguments_json", value.argumentsJson),
    stepId: protocolId("approval.step_id", value.stepId),
    preview: value.preview === undefined ? null : mapPreview(value.preview),
    deadlineRemainingMs: value.deadlineRemainingMs ?? null,
    state: approvalState(value.state),
    approvedBy:
      value.approvedBy === undefined ? null : protocolId("approval.approved_by", value.approvedBy),
  };
}

function mapPreview(value: ToolPreview): ToolPreviewDto {
  return {
    tool: protocolId("preview.tool", value.tool),
    action: protocolText("preview.action", value.action),
    target: protocolText("preview.target", value.target),
    impact: protocolText("preview.impact", value.impact),
    risk: risk(value.risk),
    sandbox: sandbox(value.sandbox),
    workingDirectory:
      value.workingDirectory === undefined
        ? null
        : protocolText("preview.working_directory", value.workingDirectory),
    requiredSecrets: value.requiredSecrets.map((secret) =>
      protocolId("preview.required_secret", secret),
    ),
  };
}
