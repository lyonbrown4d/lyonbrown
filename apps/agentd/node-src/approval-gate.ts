import type {
  KernelApprovalDecision,
  KernelApprovalGate,
  KernelApprovalRequest,
  KernelApprovalResponseStatus,
} from "@lyonbrown/kernel-pi";
import type { RuntimeApprovalPolicy } from "@lyonbrown/agent-domain";

interface PendingApproval {
  readonly requestId: string;
  readonly toolName: string;
  readonly signal: AbortSignal;
  readonly abort: () => void;
  readonly settle: (response: { readonly decision: KernelApprovalDecision }) => void;
}

export class SessionApprovalGate implements KernelApprovalGate {
  readonly #approvedTools = new Set<string>();
  #pending: PendingApproval | undefined;
  #denyRemaining = false;

  constructor(policy: RuntimeApprovalPolicy = { allowedTools: [], denyRemaining: false }) {
    for (const toolName of policy.allowedTools) this.#approvedTools.add(toolName);
    this.#denyRemaining = policy.denyRemaining;
  }

  request(request: KernelApprovalRequest): Promise<{ readonly decision: KernelApprovalDecision }> {
    if (request.signal.aborted || this.#denyRemaining) {
      return Promise.resolve({ decision: this.#denyRemaining ? "deny-remaining" : "deny-once" });
    }
    if (this.#approvedTools.has(request.toolName)) {
      return Promise.resolve({ decision: "allow-for-session" });
    }
    if (this.#pending !== undefined) return Promise.resolve({ decision: "deny-once" });
    return new Promise((resolve) => {
      const abort = (): void => {
        this.#finish(request.callId, { decision: "deny-once" });
      };
      this.#pending = {
        requestId: request.callId,
        toolName: request.toolName,
        signal: request.signal,
        abort,
        settle: resolve,
      };
      request.signal.addEventListener("abort", abort, { once: true });
      if (request.signal.aborted) abort();
    });
  }

  respond(requestId: string, decision: KernelApprovalDecision): KernelApprovalResponseStatus {
    const pending = this.#pending;
    if (pending === undefined) return "no-pending";
    if (pending.requestId !== requestId) return "mismatch";
    this.#finish(requestId, { decision });
    return "accepted";
  }

  commit(toolName: string, decision: KernelApprovalDecision): void {
    if (decision === "allow-for-session") this.#approvedTools.add(toolName);
    if (decision === "deny-remaining") this.#denyRemaining = true;
  }

  #finish(requestId: string, response: { readonly decision: KernelApprovalDecision }): void {
    const pending = this.#pending;
    if (pending?.requestId !== requestId) return;
    this.#pending = undefined;
    pending.signal.removeEventListener("abort", pending.abort);
    pending.settle(response);
  }
}
