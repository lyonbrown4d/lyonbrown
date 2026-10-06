import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, it } from "vitest";

import { createAgentRuntime } from "@lyonbrown/agent-runtime/internal/index";
import {
  CapturingKernelFactory,
  runtimeEventStore,
  sequentialEventIds,
} from "@lyonbrown/agent-runtime/test/support";

async function harness(sessionId: string) {
  const store = openEventStore({ testOnlyMemory: true });
  const kernel = new CapturingKernelFactory();
  const runtime = await createAgentRuntime({
    sessionId: asSessionId(sessionId),
    eventStore: runtimeEventStore(store),
    createKernelSession: kernel.create,
    createEventId: sequentialEventIds(),
  });
  return { store, kernel, runtime };
}

describe("runtime approval and model selection", () => {
  it("persists auto and pinned model selections while idle", async () => {
    const { store, runtime } = await harness("runtime-model-selection");

    await runtime.selectModel({ kind: "pinned", provider: "local", model: "qwen-local" });
    expect(runtime.snapshot().modelSelection).toEqual({
      kind: "pinned",
      provider: "local",
      model: "qwen-local",
    });
    await runtime.selectModel({ kind: "auto" });
    expect(runtime.snapshot().modelSelection).toEqual({ kind: "auto" });
    await expect(
      runtime.selectModel({ kind: "pinned", provider: "", model: "model" }),
    ).rejects.toThrow();

    await runtime.dispose();
    store.close();
  });

  it("correlates approval responses and maps kernel response state", async () => {
    const { store, kernel, runtime } = await harness("runtime-approval");
    let release: (() => void) | undefined;
    kernel.startBehavior = () =>
      new Promise<void>((resolve) => {
        release = resolve;
      });
    await runtime.startTurn("write");
    await kernel.emit({ type: "run_started", sessionId: "runtime-approval" });
    await kernel.emit({ type: "turn_started", turn: 1 });
    await kernel.emit({
      type: "approval_requested",
      requestId: "call-1",
      toolName: "write",
      risk: "medium",
      permissions: ["workspace.write"],
      argumentsPreview: { path: "notes.txt" },
    });

    expect(() => {
      runtime.respondApproval("other", "allow-once");
    }).toThrow(expect.objectContaining({ code: "APPROVAL_REQUEST_MISMATCH" }));
    runtime.respondApproval("call-1", "allow-for-session");
    expect(kernel.approvalResponses).toEqual([
      { requestId: "call-1", decision: "allow-for-session" },
    ]);

    kernel.approvalResponseStatus = "mismatch";
    expect(() => {
      runtime.respondApproval("call-1", "deny-once");
    }).toThrow(expect.objectContaining({ code: "APPROVAL_REQUEST_MISMATCH" }));
    kernel.approvalResponseStatus = "no-pending";
    expect(() => {
      runtime.respondApproval("call-1", "deny-remaining");
    }).toThrow(expect.objectContaining({ code: "NO_PENDING_APPROVAL" }));

    runtime.cancel();
    release?.();
    await runtime.dispose();
    store.close();
  });

  it("rejects model changes during an active turn and approvals without a pending request", async () => {
    const { store, kernel, runtime } = await harness("runtime-active-controls");
    expect(() => {
      runtime.respondApproval("missing", "deny-once");
    }).toThrow(expect.objectContaining({ code: "NO_PENDING_APPROVAL" }));
    let release: (() => void) | undefined;
    kernel.startBehavior = () =>
      new Promise<void>((resolve) => {
        release = resolve;
      });
    await runtime.startTurn("wait");
    await expect(runtime.selectModel({ kind: "auto" })).rejects.toThrow(
      expect.objectContaining({ code: "SESSION_BUSY" }),
    );

    runtime.cancel();
    release?.();
    await runtime.dispose();
    store.close();
  });
});
