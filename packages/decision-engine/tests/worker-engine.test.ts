import { describe, expect, it, vi } from "vitest";

import {
  createWorkerOpenJevDecisionEngine,
  type DecisionWorkerPool,
} from "@lyonbrown/decision-engine/internal/worker-engine";
import { injectedOpenJevConfiguration } from "@lyonbrown/decision-engine/test/model-environment";

const validAnswers = [
  {
    type: "choice" as const,
    choice: "agent",
    confidence: 0.75,
    probabilities: { chat: 0.25, agent: 0.75 },
  },
];

const request = (signal: AbortSignal) => ({
  state: "Fix the failing build.",
  signal,
  questions: [
    {
      id: "route",
      type: "choice" as const,
      instructions: "Which route should handle this task?",
      options: ["chat", "agent"] as const,
    },
  ],
});

function poolWith(run: DecisionWorkerPool["run"]): DecisionWorkerPool {
  return { run, close: vi.fn().mockResolvedValue(undefined) };
}

describe("worker-isolated OpenJev engine", () => {
  it("passes only cloneable input to the worker and validates its answer", async () => {
    const run = vi.fn().mockResolvedValue(validAnswers);
    const pool = poolWith(run);
    const configuration = injectedOpenJevConfiguration();
    const engine = createWorkerOpenJevDecisionEngine({
      pool,
      configuration,
    });
    const controller = new AbortController();

    await expect(engine.decide(request(controller.signal))).resolves.toMatchObject({
      kind: "decided",
      providerId: "open-jev",
      answers: [{ id: "route", choice: "agent" }],
    });
    expect(run).toHaveBeenCalledWith(
      {
        configuration,
        state: "Fix the failing build.",
        questions: [
          {
            type: "choice",
            instructions: "Which route should handle this task?",
            options: ["chat", "agent"],
          },
        ],
      },
      { signal: controller.signal },
    );
  });

  it("maps worker termination caused by cancellation to aborted", async () => {
    const controller = new AbortController();
    const run = vi.fn<DecisionWorkerPool["run"]>((_task, options) => {
      return new Promise((_resolve, reject) => {
        options.signal.addEventListener(
          "abort",
          () => {
            reject(new Error("worker terminated"));
          },
          { once: true },
        );
      });
    });
    const pool = poolWith(run);
    const engine = createWorkerOpenJevDecisionEngine({
      pool,
      configuration: injectedOpenJevConfiguration(),
    });

    const outcome = engine.decide(request(controller.signal));
    controller.abort();
    await expect(outcome).resolves.toEqual({ kind: "undetermined", reason: "aborted" });
  });

  it("bounds disposal by destroying the worker pool exactly once", async () => {
    const pool = poolWith(vi.fn().mockResolvedValue(validAnswers));
    const engine = createWorkerOpenJevDecisionEngine({
      pool,
      configuration: injectedOpenJevConfiguration(),
    });

    await Promise.all([engine.dispose(), engine.dispose()]);
    expect(pool.close).toHaveBeenCalledTimes(1);
    await expect(engine.decide(request(new AbortController().signal))).resolves.toEqual({
      kind: "undetermined",
      reason: "disposed",
    });
  });

  it("bounds disposal even when the worker pool never closes", async () => {
    vi.useFakeTimers();
    try {
      const pool: DecisionWorkerPool = {
        run: vi.fn().mockResolvedValue(validAnswers),
        close: vi.fn(() => new Promise<void>(() => undefined)),
      };
      const engine = createWorkerOpenJevDecisionEngine({
        pool,
        configuration: injectedOpenJevConfiguration(),
        closeTimeoutMs: 5,
      });

      const disposal = engine.dispose();
      await vi.advanceTimersByTimeAsync(5);

      await expect(disposal).resolves.toBeUndefined();
      expect(pool.close).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });
});
