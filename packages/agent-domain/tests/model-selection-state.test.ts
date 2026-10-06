import { describe, expect, it } from "vitest";

import {
  asRevision,
  asSessionId,
  createInitialAgentRuntimeState,
  parseAgentRuntimeEvent,
  reduceAgentRuntimeEvent,
} from "@lyonbrown/agent-domain/internal/index";

describe("durable model selection", () => {
  it("defaults to auto and persists a pinned Lyonbrown route", () => {
    const sessionId = asSessionId("model-selection-session");
    let state = createInitialAgentRuntimeState(sessionId);
    state = reduceAgentRuntimeEvent(state, {
      revision: asRevision(1n),
      event: { type: "session_opened", sessionId },
    });
    expect(state.modelSelection).toEqual({ kind: "auto" });

    state = reduceAgentRuntimeEvent(state, {
      revision: asRevision(2n),
      event: parseAgentRuntimeEvent({
        type: "model_selection_changed",
        sessionId,
        mode: "pinned",
        provider: "local",
        model: "qwen-local",
      }),
    });
    expect(state.modelSelection).toEqual({
      kind: "pinned",
      provider: "local",
      model: "qwen-local",
    });
  });
});
