import type { KernelApprovalRequest } from "@lyonbrown/kernel-pi";
import { describe, expect, it } from "vitest";

import { SessionApprovalGate } from "@lyonbrown/agentd/internal/approval-gate";

function request(
  callId: string,
  toolName = "write",
  signal = new AbortController().signal,
): KernelApprovalRequest {
  return {
    sessionId: "session",
    callId,
    toolName,
    risk: "medium",
    permissions: ["workspace.write"],
    argumentsPreview: { path: "notes.txt" },
    signal,
  };
}

describe("session approval gate", () => {
  it("hydrates durable session policy without reopening an approval", async () => {
    const allowed = new SessionApprovalGate({ allowedTools: ["write"], denyRemaining: false });
    await expect(allowed.request(request("call-1"))).resolves.toEqual({
      decision: "allow-for-session",
    });

    const denied = new SessionApprovalGate({ allowedTools: ["write"], denyRemaining: true });
    await expect(denied.request(request("call-2", "edit"))).resolves.toEqual({
      decision: "deny-remaining",
    });
  });

  it("correlates one pending decision and remembers session-scoped approval", async () => {
    const gate = new SessionApprovalGate();
    const pending = gate.request(request("call-1"));

    expect(gate.respond("other", "allow-once")).toBe("mismatch");
    expect(gate.respond("call-1", "allow-for-session")).toBe("accepted");
    await expect(pending).resolves.toEqual({ decision: "allow-for-session" });
    const beforeCommit = gate.request(request("call-2"));
    expect(gate.respond("call-2", "deny-once")).toBe("accepted");
    await expect(beforeCommit).resolves.toEqual({ decision: "deny-once" });

    gate.commit("write", "allow-for-session");
    await expect(gate.request(request("call-3"))).resolves.toEqual({
      decision: "allow-for-session",
    });
    expect(gate.respond("call-3", "allow-once")).toBe("no-pending");
  });

  it("denies remaining approvals and fails closed on abort", async () => {
    const gate = new SessionApprovalGate();
    const pending = gate.request(request("call-1"));
    expect(gate.respond("call-1", "deny-remaining")).toBe("accepted");
    await expect(pending).resolves.toEqual({ decision: "deny-remaining" });
    gate.commit("write", "deny-remaining");
    await expect(gate.request(request("call-2", "edit"))).resolves.toEqual({
      decision: "deny-remaining",
    });

    const fresh = new SessionApprovalGate();
    const controller = new AbortController();
    const aborted = fresh.request(request("call-3", "write", controller.signal));
    controller.abort();
    await expect(aborted).resolves.toEqual({ decision: "deny-once" });
    expect(fresh.respond("call-3", "allow-once")).toBe("no-pending");
  });
});
