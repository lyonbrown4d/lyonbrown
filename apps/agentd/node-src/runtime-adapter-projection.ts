import type { AgentRuntime } from "@lyonbrown/agent-runtime";
import {
  ActivityPhase,
  ActivitySnapshot,
  ModelSelection,
  PROTOCOL_VERSION,
  RuntimePhase,
  RuntimeSnapshot,
  TokenUsage,
  ToolActivity,
  ToolActivityState,
  TurnSnapshot,
  TurnStatus,
  validateStringId,
  validateText,
} from "@lyonbrown/protocol";

type RuntimeState = ReturnType<AgentRuntime["snapshot"]>;
type RuntimeTurn = RuntimeState["turns"][number];
type RuntimeActivity = RuntimeTurn["activities"][number];
type RuntimeTool = RuntimeActivity["tools"][number];

const MAX_UINT32 = 0xffff_ffff;
const MAX_UINT64 = 0xffff_ffff_ffff_ffffn;
const AUTO_SELECTION = ModelSelection.create({ selection: { $case: "auto", auto: true } });

export function projectRuntimeSnapshot(state: RuntimeState): RuntimeSnapshot {
  try {
    return projectTrustedState(state);
  } catch {
    throw new Error("runtime snapshot cannot be projected");
  }
}

export function assertProtocolRevision(value: bigint): bigint {
  if (typeof value !== "bigint" || value < 0n || value > MAX_UINT64) {
    throw new Error("runtime snapshot cannot be projected");
  }
  return value;
}

function projectTrustedState(state: RuntimeState): RuntimeSnapshot {
  const revision = assertProtocolRevision(state.revision);
  const sessionId = requiredIdentifier(state.sessionId);
  requireArray(state.tools);
  projectUsage(state.usage);
  const turns = requireArray(state.turns).map(projectTurn);
  const queuedFollowUps = requireArray(state.queuedFollowUps).map(requiredText);
  requireCount(state.assistantTextBytes);
  if (typeof state.outcomeUnknown !== "boolean") throw new Error("invalid state");
  if (state.outcomeUnknown && state.phase !== "failed_terminal") throw new Error("invalid state");
  return RuntimeSnapshot.create({
    protocolVersion: PROTOCOL_VERSION,
    revision,
    phase: runtimePhase(state),
    sessionId,
    turns,
    queuedFollowUps,
    lastError: projectLastError(state.lastError),
    modelSelection: AUTO_SELECTION,
  });
}

function projectTurn(turn: RuntimeTurn, index: number): TurnSnapshot {
  const turnId = `turn-${String(index + 1)}`;
  const status = turnStatus(turn.status);
  const activities = requireArray(turn.activities).map((activity) =>
    projectActivity(activity, turn.status, turnId),
  );
  if (turn.turn !== undefined) requireCount(turn.turn);
  return TurnSnapshot.create({
    turnId,
    userText: requiredText(turn.userText),
    assistantText: requiredText(turn.assistantText),
    status,
    steering: requireArray(turn.steering).map(requiredText),
    activities,
  });
}

function projectActivity(
  activity: RuntimeActivity,
  turnStatusValue: RuntimeTurn["status"],
  turnId: string,
): ActivitySnapshot {
  const step = requireCount(activity.turn);
  const tools = requireArray(activity.tools).map(projectTool);
  const usage = projectUsage(activity.usage);
  return ActivitySnapshot.create({
    stepId: `${turnId}-step-${String(step)}`,
    phase: activityPhase(turnStatusValue, activity, tools),
    assistantText: requiredText(activity.assistantText),
    usage,
    tools: [...tools],
  });
}

function projectUsage(usage: RuntimeState["usage"]): TokenUsage | undefined {
  const input = requireUint32(usage.inputTokens);
  const output = requireUint32(usage.outputTokens);
  const total = requireUint32(usage.totalTokens);
  if (input === 0 && output === 0 && total === 0) return undefined;
  return TokenUsage.create({ input, output, cacheRead: 0 });
}

function projectTool(tool: RuntimeTool): ToolActivity {
  const argumentsJson = JSON.stringify(requiredJson(tool.argumentsPreview));
  if (typeof argumentsJson !== "string") throw new Error("invalid tool arguments");
  return ToolActivity.create({
    callId: requiredIdentifier(tool.callId),
    name: requiredIdentifier(tool.toolName),
    argumentsJson,
    state: toolState(tool.status),
  });
}

function projectLastError(error: RuntimeState["lastError"]): string | undefined {
  if (error === undefined) return undefined;
  requiredIdentifier(error.code);
  return requiredText(error.message);
}

function runtimePhase(state: RuntimeState): RuntimePhase {
  switch (state.phase) {
    case "uninitialized":
      return RuntimePhase.RUNTIME_PHASE_INITIALIZING;
    case "ready":
    case "cancelled":
      return RuntimePhase.RUNTIME_PHASE_READY;
    case "running":
      if (state.tools.some((tool) => tool.status === "running")) {
        return RuntimePhase.RUNTIME_PHASE_RUNNING_TOOL;
      }
      if (state.turns.at(-1)?.assistantText) return RuntimePhase.RUNTIME_PHASE_STREAMING;
      return RuntimePhase.RUNTIME_PHASE_PREPARING;
    case "completed":
      return RuntimePhase.RUNTIME_PHASE_COMPLETED;
    case "failed":
      return RuntimePhase.RUNTIME_PHASE_FAILED_RECOVERABLE;
    case "failed_terminal":
      return RuntimePhase.RUNTIME_PHASE_FAILED_TERMINAL;
    default:
      return exhaustive(state.phase);
  }
}

function turnStatus(status: RuntimeTurn["status"]): TurnStatus {
  switch (status) {
    case "submitted":
      return TurnStatus.TURN_STATUS_PENDING;
    case "running":
      return TurnStatus.TURN_STATUS_RUNNING;
    case "completed":
      return TurnStatus.TURN_STATUS_COMPLETED;
    case "cancelled":
      return TurnStatus.TURN_STATUS_CANCELLED;
    case "failed":
      return TurnStatus.TURN_STATUS_FAILED;
    default:
      return exhaustive(status);
  }
}

function activityPhase(
  turnStatusValue: RuntimeTurn["status"],
  activity: RuntimeActivity,
  tools: readonly ToolActivity[],
): ActivityPhase {
  if (turnStatusValue === "failed") return ActivityPhase.ACTIVITY_PHASE_FAILED;
  if (turnStatusValue === "cancelled") {
    if (tools.some((tool) => tool.state === ToolActivityState.TOOL_ACTIVITY_STATE_RUNNING)) {
      return ActivityPhase.ACTIVITY_PHASE_RUNNING_TOOL;
    }
    return activity.assistantText.length === 0
      ? ActivityPhase.ACTIVITY_PHASE_PREPARING
      : ActivityPhase.ACTIVITY_PHASE_STREAMING;
  }
  switch (activity.status) {
    case "completed":
      return ActivityPhase.ACTIVITY_PHASE_COMPLETED;
    case "running":
      if (tools.some((tool) => tool.state === ToolActivityState.TOOL_ACTIVITY_STATE_RUNNING)) {
        return ActivityPhase.ACTIVITY_PHASE_RUNNING_TOOL;
      }
      return activity.assistantText.length === 0
        ? ActivityPhase.ACTIVITY_PHASE_PREPARING
        : ActivityPhase.ACTIVITY_PHASE_STREAMING;
    default:
      return exhaustive(activity.status);
  }
}

function toolState(status: RuntimeTool["status"]): ToolActivityState {
  switch (status) {
    case "running":
      return ToolActivityState.TOOL_ACTIVITY_STATE_RUNNING;
    case "succeeded":
      return ToolActivityState.TOOL_ACTIVITY_STATE_COMPLETED;
    case "failed":
    case "outcome_unknown":
      return ToolActivityState.TOOL_ACTIVITY_STATE_FAILED;
    default:
      return exhaustive(status);
  }
}

function requiredIdentifier(value: unknown): string {
  if (!validateStringId(value).ok) throw new Error("invalid identifier");
  return value as string;
}

function requiredText(value: unknown): string {
  if (!validateText(value).ok) throw new Error("invalid text");
  return value as string;
}

function requiredJson<T>(value: T | undefined): T {
  if (value === undefined) throw new Error("missing JSON value");
  return value;
}

function requireArray<T>(value: readonly T[]): readonly T[] {
  const original: readonly T[] = value;
  if (!Array.isArray(value)) throw new Error("invalid collection");
  return original;
}

function requireCount(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new Error("invalid count");
  }
  return value;
}

function requireUint32(value: unknown): number {
  const count = requireCount(value);
  if (count > MAX_UINT32) throw new Error("invalid uint32");
  return count;
}

function exhaustive(value: never): never {
  throw new Error(`unsupported runtime value: ${String(value)}`);
}
