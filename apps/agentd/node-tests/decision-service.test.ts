import type { RuntimePreKernelDecision } from "@lyonbrown/agent-runtime";
import { describe, expect, it, vi } from "vitest";

import {
  createProductionDecisionService,
  type ProductionDecisionServiceDependencies,
} from "@lyonbrown/agentd/internal/decision-service";
import type { CreateSystemOneDecisionPortOptions } from "@lyonbrown/agentd/internal/decision-adapter";

const decided: RuntimePreKernelDecision = {
  kind: "decided",
  questionVersion: "system-one-v1",
  providerId: "open-jev",
  modelId: "kev-0.6b",
  modelRevision: "5a827091b3d58fe4fdfc02a5a5b2ce364047d678",
  intent: "chat",
  risk: "low",
  requiresTools: false,
  confidence: 0.9,
  probabilities: { intent: { chat: 0.9 } },
  action: "continue_pi",
};

describe("production decision service", () => {
  it("does not choose a system-drive fallback when the model root is absent", async () => {
    const dependencies = dependencyFakes();
    const service = createProductionDecisionService({}, dependencies.values);

    await expect(
      service.port.evaluate({ input: "hello", signal: new AbortController().signal }),
    ).resolves.toMatchObject({ kind: "fallback", reason: "model_missing" });
    expect(dependencies.createStorage).not.toHaveBeenCalled();
    await service.dispose();
  });

  it("loads the configured secure bundle lazily and owns engine disposal", async () => {
    const dependencies = dependencyFakes();
    const service = createProductionDecisionService(
      { LYONBROWN_DECISION_MODEL_ROOT: "F:\\Lyonbrown\\models\\decision" },
      dependencies.values,
    );

    expect(dependencies.createStorage).not.toHaveBeenCalled();
    expect(dependencies.createAnchoredEngine).not.toHaveBeenCalled();
    await expect(
      service.port.evaluate({ input: "hello", signal: new AbortController().signal }),
    ).resolves.toEqual(decided);
    expect(dependencies.createStorage).toHaveBeenCalledWith("F:\\Lyonbrown\\models\\decision");
    expect(dependencies.createAnchoredEngine).toHaveBeenCalledOnce();
    await service.dispose();
    await service.dispose();
    expect(dependencies.engineDispose).toHaveBeenCalledOnce();
  });

  it("fails closed for invalid roots and native storage construction failures", async () => {
    const invalid = dependencyFakes();
    const invalidService = createProductionDecisionService(
      { LYONBROWN_DECISION_MODEL_ROOT: "relative/model" },
      invalid.values,
    );
    await expect(
      invalidService.port.evaluate({ input: "hello", signal: new AbortController().signal }),
    ).resolves.toMatchObject({ kind: "fallback", reason: "provider_unavailable" });

    const failed = dependencyFakes();
    failed.createStorage.mockImplementation(() => {
      throw new Error("private native failure");
    });
    const failedService = createProductionDecisionService(
      { LYONBROWN_DECISION_MODEL_ROOT: "F:\\Lyonbrown\\models\\decision" },
      failed.values,
    );
    await expect(
      failedService.port.evaluate({ input: "hello", signal: new AbortController().signal }),
    ).resolves.toMatchObject({ kind: "fallback", reason: "provider_unavailable" });
  });

  it("maps anchored model corruption to the durable runtime fallback", async () => {
    const dependencies = dependencyFakes();
    dependencies.createAnchoredEngine.mockResolvedValue({
      kind: "unavailable",
      reason: "model_corrupted",
    });
    const service = createProductionDecisionService(
      { LYONBROWN_DECISION_MODEL_ROOT: "F:\\Lyonbrown\\models\\decision" },
      dependencies.values,
    );

    await expect(
      service.port.evaluate({ input: "hello", signal: new AbortController().signal }),
    ).resolves.toMatchObject({ kind: "fallback", reason: "model_corrupted" });
    await service.dispose();
    expect(dependencies.engineDispose).not.toHaveBeenCalled();
  });

  it("returns an aborted fallback when the request is cancelled during lazy initialization", async () => {
    const dependencies = dependencyFakes();
    dependencies.createAnchoredEngine.mockImplementation(() => new Promise<never>(() => undefined));
    const service = createProductionDecisionService(
      { LYONBROWN_DECISION_MODEL_ROOT: "F:\\Lyonbrown\\models\\decision" },
      dependencies.values,
    );
    const controller = new AbortController();

    const evaluation = service.port.evaluate({ input: "hello", signal: controller.signal });
    await expect.poll(() => dependencies.createAnchoredEngine).toHaveBeenCalledOnce();
    controller.abort();

    await expect(evaluation).resolves.toMatchObject({ kind: "fallback", reason: "aborted" });
  });

  it("aborts and drains pending initialization during disposal", async () => {
    const dependencies = dependencyFakes();
    let initializationSignal: AbortSignal | undefined;
    dependencies.createAnchoredEngine.mockImplementation((_storage, options) => {
      initializationSignal = options.signal;
      return new Promise((resolve) => {
        options.signal.addEventListener(
          "abort",
          () => resolve({ kind: "unavailable", reason: "storage_unavailable" }),
          { once: true },
        );
      });
    });
    const service = createProductionDecisionService(
      { LYONBROWN_DECISION_MODEL_ROOT: "F:\\Lyonbrown\\models\\decision" },
      dependencies.values,
    );
    const evaluation = service.port.evaluate({
      input: "hello",
      signal: new AbortController().signal,
    });
    await expect.poll(() => dependencies.createAnchoredEngine).toHaveBeenCalledOnce();

    await service.dispose();

    expect(initializationSignal?.aborted).toBe(true);
    await expect(evaluation).resolves.toMatchObject({
      kind: "fallback",
      reason: "provider_unavailable",
    });
  });
});

function dependencyFakes() {
  const storage = {};
  const engineDispose = vi.fn(() => Promise.resolve());
  const evaluate = vi.fn(() => Promise.resolve(decided));
  const createStorage = vi.fn(() => storage);
  const createAnchoredEngine = vi.fn<ProductionDecisionServiceDependencies["createAnchoredEngine"]>(
    async () => ({
      kind: "configured" as const,
      engine: { decide: vi.fn(), dispose: engineDispose },
    }),
  );
  const createPort = vi.fn((options: CreateSystemOneDecisionPortOptions) => ({
    evaluate:
      options.engine === undefined
        ? () =>
            Promise.resolve({
              kind: "fallback" as const,
              questionVersion: "system-one-v1",
              reason: options.unavailableReason ?? "model_missing",
              action: "continue_pi" as const,
            })
        : evaluate,
  }));
  const values = {
    createStorage,
    createAnchoredEngine,
    createPort,
  } as unknown as ProductionDecisionServiceDependencies;
  return { values, createStorage, createAnchoredEngine, engineDispose };
}
