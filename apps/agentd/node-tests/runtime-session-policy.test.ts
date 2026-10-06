import type { AgentRuntimeState } from "@lyonbrown/agent-domain";
import { describe, expect, it } from "vitest";

import {
  MAX_SESSION_STEERING_BYTES,
  MAX_SESSION_STEERING_PER_TURN,
  MAX_SESSION_TURNS,
  MAX_SESSION_USER_BYTES,
} from "@lyonbrown/agentd/internal/runtime-limits";
import {
  followUpWithinBudget,
  shouldRotateSession,
  steeringWithinBudget,
} from "@lyonbrown/agentd/internal/runtime-session-policy";
import { stateFrom } from "@lyonbrown/agentd/test/runtime-adapter-support";

const SESSION_ID = "b".repeat(64);

function state(options: {
  readonly userTexts?: readonly string[];
  readonly steering?: readonly (readonly string[])[];
  readonly queued?: readonly string[];
  readonly running?: boolean;
}): AgentRuntimeState {
  const base = stateFrom([{ type: "session_opened" }], SESSION_ID);
  const userTexts = options.userTexts ?? [];
  return {
    ...base,
    phase: options.running === true ? "running" : "completed",
    turns: userTexts.map((userText, index) => ({
      userText,
      assistantText: "",
      status: options.running === true && index === userTexts.length - 1 ? "running" : "completed",
      turn: index + 1,
      steering: [...(options.steering?.[index] ?? [])],
      activities: [],
    })),
    queuedFollowUps: [...(options.queued ?? [])],
  };
}

describe("runtime session budget policy", () => {
  it("counts queued and consumed follow-ups against the 32-turn session budget", () => {
    const almostFull = Array.from({ length: MAX_SESSION_TURNS - 1 }, () => "turn");
    expect(followUpWithinBudget(state({ userTexts: almostFull }), "last")).toBe(true);
    expect(
      followUpWithinBudget(state({ userTexts: almostFull, queued: ["reserved"] }), "overflow"),
    ).toBe(false);
    expect(
      followUpWithinBudget(state({ userTexts: [...almostFull, "consumed"] }), "overflow"),
    ).toBe(false);
  });

  it("uses one cumulative user-byte budget for turns, steering, queue, and next input", () => {
    const prefix = "x".repeat(MAX_SESSION_USER_BYTES - 3);
    const atLimit = state({ userTexts: [prefix], steering: [["y"]], queued: ["z"] });
    expect(followUpWithinBudget(atLimit, "q")).toBe(true);
    expect(followUpWithinBudget(atLimit, "qq")).toBe(false);
    expect(shouldRotateSession(atLimit, "qq")).toBe(true);
  });

  it("rotates for reserved queued turns even before they are consumed", () => {
    const turns = Array.from({ length: MAX_SESSION_TURNS - 1 }, () => "turn");
    expect(shouldRotateSession(state({ userTexts: turns, queued: ["reserved"] }), "next")).toBe(
      true,
    );
  });

  it("bounds repeated steering per turn and by steering bytes", () => {
    const steering = Array.from({ length: MAX_SESSION_STEERING_PER_TURN }, () => "s");
    expect(
      steeringWithinBudget(
        state({ userTexts: ["active"], steering: [steering], running: true }),
        "next",
      ),
    ).toBe(false);

    const byteLimited = state({
      userTexts: ["active"],
      steering: [["s".repeat(MAX_SESSION_STEERING_BYTES)]],
      running: true,
    });
    expect(steeringWithinBudget(byteLimited, "next")).toBe(false);
  });

  it("includes steering in the session-wide user-byte budget", () => {
    const prior = "x".repeat(MAX_SESSION_USER_BYTES - 2);
    const active = state({ userTexts: [prior], steering: [["y"]], running: true });
    expect(steeringWithinBudget(active, "z")).toBe(true);
    expect(steeringWithinBudget(active, "zz")).toBe(false);
  });
});
