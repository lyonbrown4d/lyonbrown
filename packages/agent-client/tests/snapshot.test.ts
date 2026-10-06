import {
  ActivityPhase,
  ApprovalState,
  ModelLocality,
  ModelRecommendationReason,
  ModelSetupAction,
  ModelSetupPhase,
  ModelSetupSource,
  ModelTier,
  PROTOCOL_VERSION,
  RiskLevel,
  RuntimePhase,
  RuntimeSnapshot,
  SandboxStatus,
  StopReasonKind,
  ToolActivityState,
  TurnStatus,
} from "@lyonbrown/protocol";
import { describe, expect, it } from "vitest";

import { mapSnapshot } from "../src/snapshot.ts";

function fullSnapshot() {
  return RuntimeSnapshot.fromPartial({
    protocolVersion: PROTOCOL_VERSION,
    revision: 9n,
    phase: RuntimePhase.RUNTIME_PHASE_RUNNING_TOOL,
    sessionId: "session-1",
    turns: [
      {
        turnId: "turn-1",
        userText: "user",
        assistantText: "assistant",
        status: TurnStatus.TURN_STATUS_RUNNING,
        steering: ["steer"],
        activities: [
          {
            stepId: "step-1",
            phase: ActivityPhase.ACTIVITY_PHASE_RUNNING_TOOL,
            route: {
              selected: { provider: "provider", model: "model" },
              reason: "selected",
              fallbacks: [
                {
                  from: { provider: "from", model: "old" },
                  to: { provider: "to", model: "new" },
                  reason: "fallback",
                },
              ],
            },
            context: { systemTokens: 1, contextTokens: 2, dropped: 3 },
            assistantText: "working",
            usage: { input: 4, output: 5, cacheRead: 6 },
            tools: [
              {
                callId: "call-1",
                name: "shell",
                argumentsJson: '{"command":"pwd"}',
                state: ToolActivityState.TOOL_ACTIVITY_STATE_COMPLETED,
                approvedBy: "user",
                result: {
                  invocationId: "invoke-1",
                  ok: true,
                  output: "done",
                  truncated: false,
                  durationMs: 7n,
                  error: "warning",
                },
              },
            ],
            outcome: {
              stopReason: { kind: StopReasonKind.STOP_REASON_KIND_END_TURN, error: "none" },
              text: "complete",
              toolCallIds: ["call-1"],
              escalated: false,
            },
          },
        ],
      },
    ],
    queuedFollowUps: ["next"],
    pendingApproval: {
      requestId: "approval-1",
      toolName: "shell",
      argumentsJson: "{}",
      stepId: "step-1",
      preview: {
        tool: "shell",
        action: "execute",
        target: "workspace",
        impact: "read",
        risk: RiskLevel.RISK_LEVEL_READ,
        sandbox: SandboxStatus.SANDBOX_STATUS_WORKSPACE_RESTRICTED,
        workingDirectory: "C:/workspace",
        requiredSecrets: ["TOKEN"],
      },
      deadlineRemainingMs: 10n,
      state: ApprovalState.APPROVAL_STATE_PENDING,
      approvedBy: "user",
    },
    lastError: "recoverable",
    modelSelection: { selection: { $case: "auto", auto: true } },
    route: {
      selected: { provider: "provider", model: "model" },
      reason: "active",
    },
    modelSetup: {
      source: ModelSetupSource.MODEL_SETUP_SOURCE_MANAGED_PRESET,
      phase: ModelSetupPhase.MODEL_SETUP_PHASE_DOWNLOADING,
      managedModel: {
        id: "local-model",
        displayName: "Local Model",
        recommendationReason:
          ModelRecommendationReason.MODEL_RECOMMENDATION_REASON_HARDWARE_COMPATIBLE,
        expectedDownloadBytes: 11n,
        maxBytes: 12n,
        minAvailableMemoryBytes: 13n,
        sha256: "a".repeat(64),
      },
      operationId: "operation-1",
      downloadedBytes: 14n,
      totalBytes: 15n,
      failure: { code: "network", message: "retry", retryable: true },
      allowedActions: [
        ModelSetupAction.MODEL_SETUP_ACTION_CANCEL,
        ModelSetupAction.MODEL_SETUP_ACTION_RETRY,
      ],
    },
    modelCatalog: {
      models: [
        {
          route: { provider: "provider", model: "model" },
          tier: ModelTier.MODEL_TIER_WORKER,
          locality: ModelLocality.MODEL_LOCALITY_LOCAL,
          contextWindow: 16,
          maxOutputTokens: 17,
          supportsTools: true,
          supportsStreaming: true,
          supportsJson: true,
          supportsThinking: false,
          selectableForWorker: true,
        },
      ],
    },
    modelPolicy: {
      active: { selection: { $case: "auto", auto: true } },
      pending: {
        selection: {
          $case: "pinned",
          pinned: { provider: "provider", model: "next-model" },
        },
      },
    },
  });
}

describe("snapshot DTO mapping", () => {
  it("maps the complete wire snapshot into Lyonbrown DTOs", () => {
    const actual = mapSnapshot(fullSnapshot());

    expect(actual).toMatchObject({
      revision: 9n,
      phase: "running-tool",
      sessionId: "session-1",
      turns: [
        {
          turnId: "turn-1",
          status: "running",
          activities: [
            {
              stepId: "step-1",
              phase: "running-tool",
              tools: [{ state: "completed", result: { durationMs: 7n } }],
            },
          ],
        },
      ],
      pendingApproval: { requestId: "approval-1", state: "pending" },
      modelSetup: {
        phase: "downloading",
        downloadedBytes: 14n,
        managedModel: {
          displayName: "Local Model",
          recommendationReason: "hardware-compatible",
        },
      },
      modelCatalog: { models: [{ tier: "worker", locality: "local" }] },
      modelPolicy: { active: { kind: "auto" }, pending: { kind: "pinned" } },
    });
    expect(
      JSON.stringify(actual, (_key: string, value: unknown): unknown =>
        typeof value === "bigint" ? value.toString() : value,
      ),
    ).not.toContain("$type");
  });

  it("maps absent optional snapshot fields to explicit null DTO fields", () => {
    const wire = RuntimeSnapshot.fromPartial({
      protocolVersion: PROTOCOL_VERSION,
      revision: 0n,
      phase: RuntimePhase.RUNTIME_PHASE_READY,
      modelSelection: { selection: { $case: "auto", auto: true } },
    });

    expect(mapSnapshot(wire)).toMatchObject({
      sessionId: null,
      pendingApproval: null,
      lastError: null,
      route: null,
      modelSetup: null,
      modelCatalog: null,
      modelPolicy: null,
    });
  });

  it("maps absent optional nested activity fields to explicit null DTO fields", () => {
    const wire = RuntimeSnapshot.fromPartial({
      protocolVersion: PROTOCOL_VERSION,
      revision: 1n,
      phase: RuntimePhase.RUNTIME_PHASE_READY,
      modelSelection: { selection: { $case: "auto", auto: true } },
      turns: [
        {
          turnId: "turn",
          userText: "user",
          assistantText: "",
          status: TurnStatus.TURN_STATUS_PENDING,
          activities: [
            {
              stepId: "step",
              phase: ActivityPhase.ACTIVITY_PHASE_PREPARING,
              assistantText: "",
              tools: [
                {
                  callId: "call",
                  name: "tool",
                  argumentsJson: "{}",
                  state: ToolActivityState.TOOL_ACTIVITY_STATE_REQUESTED,
                },
              ],
            },
          ],
        },
      ],
      pendingApproval: {
        requestId: "request",
        toolName: "tool",
        argumentsJson: "{}",
        stepId: "step",
        state: ApprovalState.APPROVAL_STATE_PENDING,
      },
      modelSetup: {
        source: ModelSetupSource.MODEL_SETUP_SOURCE_CONFIGURED_PATH,
        phase: ModelSetupPhase.MODEL_SETUP_PHASE_READY,
      },
    });

    expect(mapSnapshot(wire)).toMatchObject({
      turns: [
        {
          activities: [
            {
              route: null,
              context: null,
              usage: null,
              outcome: null,
              tools: [{ approvedBy: null, result: null }],
            },
          ],
        },
      ],
      pendingApproval: { preview: null, deadlineRemainingMs: null, approvedBy: null },
      modelSetup: {
        managedModel: null,
        operationId: null,
        totalBytes: null,
        failure: null,
      },
    });
  });

  it("maps absent managed-model recommendation metadata to explicit null fields", () => {
    const wire = fullSnapshot();
    const managedModel = wire.modelSetup?.managedModel;
    if (managedModel === undefined) throw new Error("managed model fixture is missing");
    managedModel.displayName = undefined;
    managedModel.recommendationReason = undefined;

    expect(mapSnapshot(wire).modelSetup?.managedModel).toMatchObject({
      displayName: null,
      recommendationReason: null,
    });
  });

  it("rejects hostile managed-model recommendation metadata", () => {
    const oversized = fullSnapshot();
    const oversizedModel = oversized.modelSetup?.managedModel;
    if (oversizedModel === undefined) throw new Error("managed model fixture is missing");
    oversizedModel.displayName = "x".repeat(129);
    expect(() => mapSnapshot(oversized)).toThrow(/display_name/u);

    const terminalControl = fullSnapshot();
    const terminalControlModel = terminalControl.modelSetup?.managedModel;
    if (terminalControlModel === undefined) throw new Error("managed model fixture is missing");
    terminalControlModel.displayName = "trusted\u001b[31m";
    expect(() => mapSnapshot(terminalControl)).toThrow(/display_name/u);

    const unknownReason = fullSnapshot();
    const unknownModel = unknownReason.modelSetup?.managedModel;
    if (unknownModel === undefined) throw new Error("managed model fixture is missing");
    unknownModel.recommendationReason = 99 as ModelRecommendationReason;
    expect(() => mapSnapshot(unknownReason)).toThrow(/unknown enum/u);
  });

  it("rejects incompatible, malformed, and internally conflicting snapshots", () => {
    const incompatible = fullSnapshot();
    incompatible.protocolVersion += 1;
    expect(() => mapSnapshot(incompatible)).toThrow(/protocol version/u);

    const malformedJson = fullSnapshot();
    const tool = malformedJson.turns[0]?.activities[0]?.tools[0];
    if (tool !== undefined) tool.argumentsJson = "{";
    expect(() => mapSnapshot(malformedJson)).toThrow(/valid JSON/u);

    const conflict = fullSnapshot();
    conflict.modelPolicy = {
      $type: "lyonbrown.agent.v1.ModelPolicy",
      active: {
        $type: "lyonbrown.agent.v1.ModelSelection",
        selection: {
          $case: "pinned",
          pinned: { $type: "lyonbrown.agent.v1.ModelRoute", provider: "p", model: "m" },
        },
      },
      pending: undefined,
    };
    expect(() => mapSnapshot(conflict)).toThrow(/conflicts/u);
  });
});
