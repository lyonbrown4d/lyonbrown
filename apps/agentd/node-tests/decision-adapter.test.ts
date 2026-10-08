import type { DecisionEnginePort, DecisionRequest } from "@lyonbrown/decision-engine";
import { describe, expect, it, vi } from "vitest";

import { createSystemOneDecisionPort } from "@lyonbrown/agentd/internal/decision-adapter";

describe("System One runtime decision adapter", () => {
  it("maps closed OpenJev answers into the Lyonbrown pre-kernel contract", async () => {
    const decide = vi.fn(async (_request: DecisionRequest) => ({
      kind: "decided" as const,
      providerId: "open-jev",
      modelId: "kev-0.6b",
      modelRevision: "5a827091b3d58fe4fdfc02a5a5b2ce364047d678",
      answers: [
        {
          id: "intent",
          type: "choice" as const,
          choice: "code-change",
          confidence: 0.8,
          probabilities: { chat: 0.1, "code-change": 0.8, "system-operation": 0.1 },
        },
        {
          id: "risk",
          type: "score" as const,
          score: 1,
          normalized: 0.5,
          level: "medium",
          confidence: 0.7,
          probabilities: { low: 0.2, medium: 0.7, high: 0.1 },
        },
        {
          id: "requires_tools",
          type: "noul" as const,
          answer: true,
          probability: 0.9,
          confidence: 0.9,
        },
      ],
    }));
    const port = createSystemOneDecisionPort({ engine: engine(decide) });

    await expect(
      port.evaluate({ input: "change the file", signal: new AbortController().signal }),
    ).resolves.toEqual({
      kind: "decided",
      questionVersion: "system-one-v1",
      providerId: "open-jev",
      modelId: "kev-0.6b",
      modelRevision: "5a827091b3d58fe4fdfc02a5a5b2ce364047d678",
      intent: "code-change",
      risk: "medium",
      requiresTools: true,
      confidence: 0.7,
      probabilities: {
        intent: { chat: 0.1, "code-change": 0.8, "system-operation": 0.1 },
        risk: { low: 0.2, medium: 0.7, high: 0.1 },
        requiresTools: { false: 0.1, true: 0.9 },
      },
      action: "continue_pi",
    });
    expect(decide.mock.calls[0]?.[0]).toMatchObject({
      state: "change the file",
      questions: [
        { id: "intent", type: "choice" },
        { id: "risk", type: "score" },
        { id: "requires_tools", type: "noul" },
      ],
    });
  });

  it("uses explicit safe fallbacks for absent and malformed providers", async () => {
    await expect(
      createSystemOneDecisionPort({ unavailableReason: "model_missing" }).evaluate({
        input: "hello",
        signal: new AbortController().signal,
      }),
    ).resolves.toEqual({
      kind: "fallback",
      questionVersion: "system-one-v1",
      reason: "model_missing",
      action: "continue_pi",
    });

    const port = createSystemOneDecisionPort({
      engine: engine(async () => ({
        kind: "decided",
        providerId: "open-jev",
        modelId: "kev-0.6b",
        modelRevision: "5a827091b3d58fe4fdfc02a5a5b2ce364047d678",
        answers: [],
      })),
    });
    await expect(
      port.evaluate({ input: "hello", signal: new AbortController().signal }),
    ).resolves.toMatchObject({ kind: "fallback", reason: "invalid_response" });
  });

  it.each([
    ["invalid_request", false, "invalid_response"],
    ["malformed_response", false, "invalid_response"],
    ["provider_unavailable", false, "provider_unavailable"],
    ["disposed", false, "provider_unavailable"],
    ["aborted", false, "provider_unavailable"],
    ["aborted", true, "aborted"],
  ] as const)(
    "maps provider outcome %s with aborted=%s to %s",
    async (reason, aborted, expected) => {
      const controller = new AbortController();
      if (aborted) controller.abort();
      const port = createSystemOneDecisionPort({
        engine: engine(async () => ({ kind: "undetermined", reason })),
      });

      await expect(
        port.evaluate({ input: "hello", signal: controller.signal }),
      ).resolves.toMatchObject({ kind: "fallback", reason: expected });
    },
  );
});

function engine(decide: DecisionEnginePort["decide"]): DecisionEnginePort {
  return { decide, dispose: () => Promise.resolve() };
}
