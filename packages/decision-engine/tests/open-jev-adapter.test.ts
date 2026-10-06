import { describe, expect, it, vi } from "vitest";

import { createOpenJevDecisionEngine } from "@lyonbrown/decision-engine/internal/open-jev-adapter";
import type { OpenJevRuntime } from "@lyonbrown/decision-engine/internal/open-jev-runtime";

const request = (signal: AbortSignal) => ({
  state: "The production API is down and customers cannot check out.",
  signal,
  questions: [
    {
      id: "route",
      type: "choice" as const,
      instructions: "Which route should handle this task?",
      options: ["chat", "agent"] as const,
    },
    {
      id: "urgency",
      type: "score" as const,
      instructions: "How urgent is this task?",
      levels: ["low", "medium", "high"] as const,
    },
    {
      id: "review",
      type: "noul" as const,
      instructions: "Does this task require human review?",
    },
  ],
});

const validAnswers = [
  {
    type: "choice" as const,
    choice: "agent",
    confidence: 0.8,
    probabilities: { chat: 0.2, agent: 0.8 },
  },
  {
    type: "score" as const,
    score: 1.8,
    normalized: 0.9,
    level: "high",
    confidence: 0.8,
    probabilities: { low: 0.05, medium: 0.15, high: 0.8 },
  },
  { type: "noul" as const, answer: true, probability: 0.9, confidence: 0.9 },
];

function runtimeWith(
  decide: OpenJevRuntime["decide"] = vi.fn().mockResolvedValue(validAnswers),
): OpenJevRuntime {
  return { decide, dispose: vi.fn().mockResolvedValue(undefined) };
}

describe("OpenJev adapter", () => {
  it("maps closed questions and returns only Lyonbrown-owned answers", async () => {
    const runtime = runtimeWith();
    const engine = createOpenJevDecisionEngine({
      runtime,
      modelId: "kev-0.6b",
      modelRevision: "revision-a",
    });

    await expect(engine.decide(request(new AbortController().signal))).resolves.toEqual({
      kind: "decided",
      providerId: "open-jev",
      modelId: "kev-0.6b",
      modelRevision: "revision-a",
      answers: [
        {
          id: "route",
          type: "choice",
          choice: "agent",
          confidence: 0.8,
          probabilities: { chat: 0.2, agent: 0.8 },
        },
        {
          id: "urgency",
          type: "score",
          score: 1.8,
          normalized: 0.9,
          level: "high",
          confidence: 0.8,
          probabilities: { low: 0.05, medium: 0.15, high: 0.8 },
        },
        {
          id: "review",
          type: "noul",
          answer: true,
          probability: 0.9,
          confidence: 0.9,
        },
      ],
    });

    expect(runtime.decide).toHaveBeenCalledWith(
      request(new AbortController().signal).state,
      [
        {
          type: "choice",
          instructions: "Which route should handle this task?",
          options: ["chat", "agent"],
        },
        {
          type: "score",
          instructions: "How urgent is this task?",
          options: ["low", "medium", "high"],
        },
        { type: "noul", instructions: "Does this task require human review?" },
      ],
      { truncation: "error" },
    );
  });

  it("fails closed when a provider returns an option outside the declared set", async () => {
    const hostile = [...validAnswers];
    hostile[0] = { ...validAnswers[0], choice: "bypass" } as (typeof validAnswers)[number];
    const engine = createOpenJevDecisionEngine({
      runtime: runtimeWith(vi.fn().mockResolvedValue(hostile)),
      modelId: "kev-0.6b",
      modelRevision: "revision-a",
    });

    await expect(engine.decide(request(new AbortController().signal))).resolves.toEqual({
      kind: "undetermined",
      reason: "malformed_response",
    });
  });

  it("returns aborted and discards a late provider result", async () => {
    let resolveDecision: ((value: typeof validAnswers) => void) | undefined;
    const pending = new Promise<typeof validAnswers>((resolve) => {
      resolveDecision = resolve;
    });
    const engine = createOpenJevDecisionEngine({
      runtime: runtimeWith(() => pending),
      modelId: "kev-0.6b",
      modelRevision: "revision-a",
    });
    const controller = new AbortController();

    const outcome = engine.decide(request(controller.signal));
    controller.abort();
    await expect(outcome).resolves.toEqual({ kind: "undetermined", reason: "aborted" });
    resolveDecision?.(validAnswers);
    await Promise.resolve();
  });

  it("sanitizes provider failures and disposes the runtime once", async () => {
    const runtime = runtimeWith(vi.fn().mockRejectedValue(new Error("C:\\secret\\model.onnx")));
    const engine = createOpenJevDecisionEngine({
      runtime,
      modelId: "kev-0.6b",
      modelRevision: "revision-a",
    });

    await expect(engine.decide(request(new AbortController().signal))).resolves.toEqual({
      kind: "undetermined",
      reason: "provider_unavailable",
    });
    await Promise.all([engine.dispose(), engine.dispose()]);
    expect(runtime.dispose).toHaveBeenCalledTimes(1);
  });
});
