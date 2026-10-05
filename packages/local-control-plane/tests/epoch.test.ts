import { describe, expect, it } from "vitest";
import {
  StaleServerEpochError,
  bindDiscoveredEpoch,
  createDiscoveryDescriptor,
  fenceEpochAtEnqueue,
  fenceEpochAtExecution,
} from "../src/index.ts";

const SCOPE_ID = "ef".repeat(32);

function descriptor() {
  return createDiscoveryDescriptor({ endpoint: "127.0.0.1:41000", pid: 101, scopeId: SCOPE_ID });
}

describe("epoch binding", () => {
  it("returns an immutable binding that does not follow descriptor replacement", () => {
    const oldDescriptor = descriptor();
    const binding = bindDiscoveredEpoch(oldDescriptor);
    const replacement = descriptor();

    expect(Object.isFrozen(binding)).toBe(true);
    expect(binding.serverInstanceId).toBe(oldDescriptor.serverInstanceId);
    expect(binding.serverInstanceId).not.toBe(replacement.serverInstanceId);
    expect(binding.endpoint).toBe(oldDescriptor.endpoint);
  });

  it("fences a stale command at enqueue and execution with a typed error", () => {
    const binding = bindDiscoveredEpoch(descriptor());
    const replacement = descriptor();

    for (const fence of [fenceEpochAtEnqueue, fenceEpochAtExecution]) {
      expect(() => {
        fence(binding, replacement.serverInstanceId);
      }).toThrow(StaleServerEpochError);
      expect(() => {
        fence(binding, replacement.serverInstanceId);
      }).toThrow(
        expect.objectContaining({
          code: "STALE_SERVER_EPOCH",
          expectedServerInstanceId: binding.serverInstanceId,
          actualServerInstanceId: replacement.serverInstanceId,
        }),
      );
      expect(() => {
        fence(binding, binding.serverInstanceId);
      }).not.toThrow();
    }
  });
});
