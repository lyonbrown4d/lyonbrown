import { expect, test } from "vitest";
import { commandFor } from "@lyonbrown/agent-desktop/internal/main/actions";
import { projectSnapshot } from "@lyonbrown/agent-desktop/internal/main/projection";
import { snapshot } from "@lyonbrown/agent-desktop/test/support";
import type { DecisionModelDto } from "@lyonbrown/agent-client";
const decision: DecisionModelDto = {
  providerId: "open-jev",
  modelId: "kev",
  modelRevision: "a".repeat(40),
  advisoryOnly: true,
  phase: "required",
  operationId: null,
  downloadedBytes: 0n,
  totalBytes: 100n,
  failure: null,
  allowedActions: ["download", "retry"],
};
const token = "fixture:1";
test("maps current pending approvals to once-only backend decisions", () => {
  const state = snapshot({
    phase: "awaiting-approval",
    pendingApproval: {
      requestId: "request",
      toolName: "write",
      argumentsJson: "{}",
      stepId: "step",
      preview: null,
      deadlineRemainingMs: null,
      state: "pending",
      approvedBy: null,
    },
  });
  expect(commandFor({ type: "approve", token }, state)).toEqual({
    type: "respond-approval",
    requestId: "request",
    decision: "allow-once",
  });
  expect(commandFor({ type: "deny", token }, state)).toEqual({
    type: "respond-approval",
    requestId: "request",
    decision: "deny-once",
  });
  expect(commandFor({ type: "cancel", token }, state)).toEqual({ type: "cancel-turn" });
  expect(projectSnapshot(state, "reused").approval?.tool).toBe("write");
});
test("uses daemon-advertised identities and actions for optional decision installation", () => {
  const state = snapshot({ decisionModel: decision });
  expect(
    commandFor({ type: "model", purpose: "decision", action: "download", token }, state),
  ).toEqual({ type: "start-model-download", modelId: "kev", purpose: "decision" });
  expect(() =>
    commandFor({ type: "model", purpose: "decision", action: "cancel", token }, state),
  ).toThrow();
  const active = snapshot({
    decisionModel: {
      ...decision,
      phase: "downloading",
      operationId: "operation",
      allowedActions: ["cancel"],
    },
  });
  expect(
    commandFor({ type: "model", purpose: "decision", action: "cancel", token }, active),
  ).toEqual({ type: "cancel-model-download", operationId: "operation", purpose: "decision" });
  expect(projectSnapshot(active, "spawned").models[0]?.totalBytes).toBe("100");
});
test("rejects unavailable model and chat actions", () => {
  expect(() =>
    commandFor({ type: "model", purpose: "chat", action: "retry", token }, snapshot()),
  ).toThrow();
  expect(() =>
    commandFor({ type: "send", text: "x", token }, snapshot({ phase: "model-setup-required" })),
  ).toThrow();
  expect(() =>
    commandFor(
      { type: "model", purpose: "decision", action: "cancel", token },
      snapshot({ decisionModel: { ...decision, allowedActions: ["cancel"] } }),
    ),
  ).toThrow();
});
test("maps chat download and cancel with verified catalog identity", () => {
  const managedModel = {
    id: "qwen",
    displayName: "Qwen",
    recommendationReason: "hardware-compatible" as const,
    expectedDownloadBytes: 100n,
    maxBytes: 100n,
    minAvailableMemoryBytes: 0n,
    sha256: "a".repeat(64),
  };
  const modelSetup = {
    source: "managed-preset" as const,
    phase: "required" as const,
    managedModel,
    operationId: null,
    downloadedBytes: 0n,
    totalBytes: 100n,
    failure: null,
    allowedActions: ["download" as const],
  };
  const state = snapshot({ modelSetup });
  expect(commandFor({ type: "model", purpose: "chat", action: "download", token }, state)).toEqual({
    type: "start-model-download",
    modelId: "qwen",
    purpose: "chat",
  });
  expect(projectSnapshot(state, "reused").models[0]?.title).toBe("Qwen");
  expect(() =>
    commandFor(
      { type: "model", purpose: "chat", action: "retry", token },
      snapshot({ modelSetup: { ...modelSetup, managedModel: null, allowedActions: ["retry"] } }),
    ),
  ).toThrow();
});
