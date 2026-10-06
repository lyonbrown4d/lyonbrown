import type { AgentRuntimeState } from "@lyonbrown/agent-domain";
import { ActivityPhase, RuntimePhase, ToolActivityState, TurnStatus } from "@lyonbrown/protocol";
import { describe, expect, test } from "vitest";

import { createRuntimeHostAdapter } from "@lyonbrown/agentd/internal/runtime-adapter";
import {
  FakeAgentRuntime,
  openedState,
  stateFrom,
} from "@lyonbrown/agentd/test/runtime-adapter-support";

describe("runtime snapshot projection", () => {
  test("projects recovered terminal, usage, and tool state without inventing optional data", async () => {
    const state = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "inspect" },
      { type: "run_started" },
      { type: "turn_started", turn: 2 },
      { type: "assistant_text_delta", delta: "partial" },
      { type: "usage_recorded", inputTokens: 3, outputTokens: 2, totalTokens: 5 },
      {
        type: "tool_started",
        callId: "call-1",
        toolName: "read_file",
        argumentsPreview: { path: "notes.txt" },
      },
      {
        type: "tool_outcome_unknown",
        callId: "call-1",
        toolName: "read_file",
        code: "TOOL_TIMEOUT",
        message: "tool outcome unknown",
      },
      { type: "run_failed", code: "TOOL_TIMEOUT", message: "tool outcome unknown" },
    ]);

    const snapshot = await createRuntimeHostAdapter(new FakeAgentRuntime(state)).snapshot();

    expect(snapshot).toMatchObject({
      protocolVersion: 1,
      revision: 9n,
      phase: RuntimePhase.RUNTIME_PHASE_FAILED_TERMINAL,
      sessionId: "adapter-session",
      lastError: "tool outcome unknown",
      queuedFollowUps: [],
      pendingApproval: undefined,
      modelSelection: { selection: { $case: "auto", auto: true } },
      route: undefined,
      modelSetup: undefined,
      modelCatalog: undefined,
      modelPolicy: undefined,
      turns: [
        {
          turnId: "turn-1",
          userText: "inspect",
          assistantText: "partial",
          status: TurnStatus.TURN_STATUS_FAILED,
          steering: [],
          activities: [
            {
              stepId: "turn-1-step-2",
              phase: ActivityPhase.ACTIVITY_PHASE_FAILED,
              assistantText: "partial",
              usage: { input: 3, output: 2, cacheRead: 0 },
              route: undefined,
              context: undefined,
              outcome: undefined,
              tools: [
                {
                  callId: "call-1",
                  name: "read_file",
                  argumentsJson: '{"path":"notes.txt"}',
                  state: ToolActivityState.TOOL_ACTIVITY_STATE_FAILED,
                  approvedBy: undefined,
                  result: undefined,
                },
              ],
            },
          ],
        },
      ],
    });
  });

  test.each([
    ["uninitialized", RuntimePhase.RUNTIME_PHASE_INITIALIZING],
    ["ready", RuntimePhase.RUNTIME_PHASE_READY],
    ["running", RuntimePhase.RUNTIME_PHASE_PREPARING],
    ["completed", RuntimePhase.RUNTIME_PHASE_COMPLETED],
    ["cancelled", RuntimePhase.RUNTIME_PHASE_READY],
    ["failed", RuntimePhase.RUNTIME_PHASE_FAILED_RECOVERABLE],
    ["failed_terminal", RuntimePhase.RUNTIME_PHASE_FAILED_TERMINAL],
  ] as const)("maps the %s runtime phase", async (phase, expected) => {
    const runtime = new FakeAgentRuntime({ ...openedState(), phase } as AgentRuntimeState);
    const snapshot = await createRuntimeHostAdapter(runtime).snapshot();
    expect(snapshot.phase).toBe(expected);
  });

  test("derives streaming and running-tool phases from current evidence", async () => {
    const streaming = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "question" },
      { type: "run_started" },
      { type: "turn_started", turn: 1 },
      { type: "assistant_text_delta", delta: "answer" },
    ]);
    const runningTool = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "question" },
      { type: "run_started" },
      { type: "turn_started", turn: 1 },
      {
        type: "tool_started",
        callId: "call-1",
        toolName: "shell",
        argumentsPreview: { command: "pwd" },
      },
    ]);

    expect(createRuntimeHostAdapter(new FakeAgentRuntime(streaming)).snapshot()).toMatchObject({
      phase: RuntimePhase.RUNTIME_PHASE_STREAMING,
    });
    expect(createRuntimeHostAdapter(new FakeAgentRuntime(runningTool)).snapshot()).toMatchObject({
      phase: RuntimePhase.RUNTIME_PHASE_RUNNING_TOOL,
    });
  });

  test("keeps a submitted turn but omits an activity until the domain has a step number", async () => {
    const state = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "question" },
    ]);
    const snapshot = await createRuntimeHostAdapter(new FakeAgentRuntime(state)).snapshot();

    expect(snapshot.turns).toEqual([
      expect.objectContaining({
        turnId: "turn-1",
        status: TurnStatus.TURN_STATUS_PENDING,
        activities: [],
      }),
    ]);
  });

  test("projects durable steering, queued follow-ups, and usage on the owning activity", async () => {
    const state = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "first" },
      { type: "run_started" },
      { type: "turn_started", turn: 1 },
      { type: "steering_submitted", text: "focus" },
      { type: "usage_recorded", inputTokens: 2, outputTokens: 1, totalTokens: 3 },
      { type: "turn_completed", turn: 1 },
      { type: "run_completed" },
      { type: "user_message_submitted", text: "second" },
      { type: "run_started" },
      { type: "turn_started", turn: 1 },
      { type: "usage_recorded", inputTokens: 5, outputTokens: 3, totalTokens: 8 },
      { type: "follow_up_queued", text: "third" },
    ]);

    const snapshot = await createRuntimeHostAdapter(new FakeAgentRuntime(state)).snapshot();

    expect(snapshot.queuedFollowUps).toEqual(["third"]);
    expect(snapshot.turns[0]).toMatchObject({
      steering: ["focus"],
      activities: [{ usage: { input: 2, output: 1, cacheRead: 0 } }],
    });
    expect(snapshot.turns[1]).toMatchObject({
      steering: [],
      activities: [{ usage: { input: 5, output: 3, cacheRead: 0 } }],
    });
  });

  test.each([
    [[], TurnStatus.TURN_STATUS_PENDING],
    [[{ type: "run_started" }, { type: "turn_started", turn: 1 }], TurnStatus.TURN_STATUS_RUNNING],
    [
      [{ type: "run_started" }, { type: "turn_started", turn: 1 }, { type: "run_completed" }],
      TurnStatus.TURN_STATUS_COMPLETED,
    ],
    [
      [{ type: "run_started" }, { type: "turn_started", turn: 1 }, { type: "run_cancelled" }],
      TurnStatus.TURN_STATUS_CANCELLED,
    ],
    [
      [
        { type: "run_started" },
        { type: "turn_started", turn: 1 },
        { type: "run_failed", code: "MODEL_ERROR", message: "failed" },
      ],
      TurnStatus.TURN_STATUS_FAILED,
    ],
  ] as const)("maps every turn status %#", async (tail, expected) => {
    const state = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "question" },
      ...tail,
    ]);
    const snapshot = await createRuntimeHostAdapter(new FakeAgentRuntime(state)).snapshot();
    expect(snapshot.turns[0]?.status).toBe(expected);
  });

  test("keeps a cancelled empty activity in preparing phase", async () => {
    const state = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "question" },
      { type: "run_started" },
      { type: "turn_started", turn: 1 },
      { type: "run_cancelled" },
    ]);

    const snapshot = await createRuntimeHostAdapter(new FakeAgentRuntime(state)).snapshot();
    expect(snapshot.turns[0]?.activities[0]?.phase).toBe(ActivityPhase.ACTIVITY_PHASE_PREPARING);
  });

  test.each([
    ["running", undefined, ToolActivityState.TOOL_ACTIVITY_STATE_RUNNING],
    ["succeeded", undefined, ToolActivityState.TOOL_ACTIVITY_STATE_COMPLETED],
    ["failed", "TOOL_FAILED", ToolActivityState.TOOL_ACTIVITY_STATE_FAILED],
    ["outcome_unknown", "TOOL_TIMEOUT", ToolActivityState.TOOL_ACTIVITY_STATE_FAILED],
  ] as const)("maps the %s tool status", async (status, code, expected) => {
    const terminal =
      status === "running"
        ? []
        : status === "succeeded"
          ? [{ type: "tool_succeeded", callId: "call-1", toolName: "read", content: "ok" } as const]
          : [
              {
                type: status === "failed" ? "tool_failed" : "tool_outcome_unknown",
                callId: "call-1",
                toolName: "read",
                code,
                message: "tool failed",
              } as const,
            ];
    const state = stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "question" },
      { type: "run_started" },
      { type: "turn_started", turn: 1 },
      {
        type: "tool_started",
        callId: "call-1",
        toolName: "read",
        argumentsPreview: { path: "notes.txt" },
      },
      ...terminal,
    ]);
    const snapshot = await createRuntimeHostAdapter(new FakeAgentRuntime(state)).snapshot();
    expect(snapshot.turns[0]?.activities[0]?.tools[0]?.state).toBe(expected);
  });

  test.each([
    ["phase", { phase: "future-phase" }],
    [
      "turn status",
      { turns: [{ ...stateFrom([{ type: "session_opened" }]).turns[0], status: "new" }] },
    ],
    ["unsafe usage", { usage: { inputTokens: 2 ** 32, outputTokens: 0, totalTokens: 2 ** 32 } }],
    ["revision", { revision: -1n }],
    ["oversized identifier", { sessionId: "x".repeat(129) }],
    [
      "optional turn",
      { turns: [{ userText: "x", assistantText: "", status: "running", turn: -1 }] },
    ],
    [
      "tool status",
      {
        phase: "running",
        turns: [{ userText: "x", assistantText: "", status: "running", turn: 1 }],
        tools: [
          {
            callId: "call-1",
            toolName: "read",
            status: "future",
            argumentsPreview: {},
          },
        ],
      },
    ],
  ])("fails closed for invalid %s", async (_label, changes) => {
    const state = { ...openedState(), ...changes } as unknown as AgentRuntimeState;
    const host = createRuntimeHostAdapter(new FakeAgentRuntime(state));
    await expect(Promise.resolve().then(() => host.snapshot())).rejects.toThrow(
      "runtime snapshot cannot be projected",
    );
  });
});
