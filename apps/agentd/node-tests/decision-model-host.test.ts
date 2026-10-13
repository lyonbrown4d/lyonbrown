import { openEventStore } from "@lyonbrown/event-store";
import { DEFAULT_LOCAL_MODEL, type LocalModelService } from "@lyonbrown/local-model";
import { DecisionModelPhase, RuntimePhase } from "@lyonbrown/protocol";
import { describe, expect, it, vi } from "vitest";

import { createModelAwareRuntime } from "@lyonbrown/agentd/internal/model-aware-runtime";
import {
  initialDecisionModelStatus,
  type DecisionModelStatus,
} from "@lyonbrown/agentd/internal/decision-model-state";

describe("decision state alongside chat runtime", () => {
  it("publishes decision-only revisions and routes installs separately from chat", async () => {
    let listener: (value: DecisionModelStatus) => void = () => undefined;
    let state: DecisionModelStatus = {
      ...initialDecisionModelStatus("required"),
      allowedActions: ["download"],
    };
    const unsubscribe = vi.fn();
    const start = vi.fn(() => ({ ok: true }) as const);
    const cancel = vi.fn(() => ({ ok: true }) as const);
    const startChat = vi.fn(() => ({ kind: "started", operationId: "chat-op" }) as const);
    const models = {
      status: () => ({
        model: DEFAULT_LOCAL_MODEL,
        phase: "setup-required",
        operationId: null,
        downloadedBytes: 0,
        totalBytes: null,
        failure: null,
        selectionReason: "conservative-fallback",
        allowedActions: ["download"],
      }),
      subscribe: () => () => undefined,
      startDownload: startChat,
      dispose: async () => undefined,
    } as unknown as LocalModelService;
    const runtime = createModelAwareRuntime({
      sessionId: "decision-state-test",
      modelService: models,
      eventStore: openEventStore({ testOnlyMemory: true }),
      decisionModel: {
        status: () => state,
        start,
        cancel,
        subscribe: (next) => {
          listener = next;
          return unsubscribe;
        },
      },
    });
    try {
      const before = await runtime.host.snapshot();
      state = {
        ...state,
        phase: "downloading",
        operationId: "op",
        downloadedBytes: 10,
        allowedActions: ["cancel"],
      };
      listener(state);
      const after = await runtime.host.snapshot();
      expect(after.revision).toBeGreaterThan(before.revision);
      expect(after.phase).toBe(RuntimePhase.RUNTIME_PHASE_MODEL_SETUP_REQUIRED);
      expect(after.decisionModel?.phase).toBe(DecisionModelPhase.DECISION_MODEL_PHASE_DOWNLOADING);
      expect(after.modelSetup?.managedModel?.id).toBe(DEFAULT_LOCAL_MODEL.id);
      await runtime.host.execute({
        kind: "start-model-download",
        purpose: "decision",
        modelId: "kev-0.6b",
      });
      await runtime.host.execute({
        kind: "cancel-model-download",
        purpose: "decision",
        operationId: "op",
      });
      expect(start).toHaveBeenCalledWith("kev-0.6b");
      expect(cancel).toHaveBeenCalledWith("op");
      expect(startChat).not.toHaveBeenCalled();
      await runtime.host.execute({ kind: "start-model-download", modelId: DEFAULT_LOCAL_MODEL.id });
      expect(startChat).toHaveBeenCalledOnce();
      expect(runtime.host.capabilities()).toContain("decision-model-install.v1");
    } finally {
      await runtime.close();
    }
    expect(unsubscribe).toHaveBeenCalledOnce();
  });
});
