import { describe, expect, it, vi } from "vitest";

import {
  initialDecisionModelStatus,
  type DecisionModelStatus,
} from "@lyonbrown/agentd/internal/decision-model-state";
import { createManagedDecisionService } from "@lyonbrown/agentd/internal/managed-decision-service";

describe("decision availability without chat restart", () => {
  it("records safe fallback before installation and switches the stable port after weights are verified", async () => {
    let state: DecisionModelStatus = initialDecisionModelStatus("required");
    const dispose = vi.fn(async () => undefined);
    const evaluate = vi.fn(
      async () =>
        ({
          kind: "fallback",
          questionVersion: "system-one-v1",
          reason: "provider_unavailable",
          action: "continue_pi",
        }) as const,
    );
    const createService = vi.fn(() => ({ port: { evaluate }, dispose }));
    const controller = {
      status: () => state,
      start: () => ({ ok: true }) as const,
      cancel: () => ({ ok: true }) as const,
      subscribe: () => () => undefined,
      dispose: vi.fn(async () => undefined),
    };
    const service = createManagedDecisionService(
      {},
      { createController: () => controller, createService },
    );
    const request = { input: "hello", signal: new AbortController().signal };
    await expect(service.port.evaluate(request)).resolves.toMatchObject({
      kind: "fallback",
      reason: "model_missing",
      action: "continue_pi",
    });
    expect(createService).not.toHaveBeenCalled();
    state = initialDecisionModelStatus("configured");
    await service.port.evaluate(request);
    await service.port.evaluate(request);
    expect(createService).toHaveBeenCalledOnce();
    expect(evaluate).toHaveBeenCalledTimes(2);
    await service.dispose();
    await expect(service.port.evaluate(request)).resolves.toMatchObject({
      reason: "provider_unavailable",
    });
    expect(evaluate).toHaveBeenCalledTimes(2);
    expect(dispose).toHaveBeenCalledOnce();
  });

  it.each(["unavailable", "failed"] as const)(
    "falls back safely for %s state without loading a model",
    async (phase) => {
      const state = {
        ...initialDecisionModelStatus(phase),
        ...(phase === "failed"
          ? {
              failure: {
                code: "decision_model_corrupted",
                message: "Setup failed",
                retryable: true,
              },
            }
          : {}),
      };
      const createService = vi.fn();
      const controller = {
        status: () => state,
        start: () => ({ ok: false, error: { code: "MODEL_DOWNLOAD_UNAVAILABLE" } }) as const,
        cancel: () => ({ ok: false, error: { code: "NO_ACTIVE_MODEL_DOWNLOAD" } }) as const,
        subscribe: () => () => undefined,
        dispose: vi.fn(async () => undefined),
      };
      const service = createManagedDecisionService(
        {},
        { createController: () => controller, createService },
      );
      await expect(
        service.port.evaluate({ input: "hello", signal: AbortSignal.abort() }),
      ).resolves.toMatchObject({
        reason: phase === "failed" ? "model_corrupted" : "provider_unavailable",
      });
      await service.dispose();
      expect(createService).not.toHaveBeenCalled();
    },
  );
});
