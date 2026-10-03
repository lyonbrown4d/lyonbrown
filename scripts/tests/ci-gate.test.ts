import { describe, expect, test, vi } from "vitest";

import { CI_GATES, runCiGate } from "../ci-gate-lib.js";

describe("CI gate orchestration", () => {
  test("runs every gate in the required order", async () => {
    const calls: string[] = [];

    await runCiGate((gate) => {
      calls.push(gate);
      return Promise.resolve();
    });

    expect(calls).toEqual(["preflight", "lint", "typecheck", "test", "build"]);
    expect(calls).toEqual(CI_GATES);
  });

  test("stops and forwards the first failure", async () => {
    const failure = new Error("typecheck failed");
    const runner = vi.fn((gate: string) => {
      if (gate === "typecheck") {
        return Promise.reject(failure);
      }
      return Promise.resolve();
    });

    await expect(runCiGate(runner)).rejects.toBe(failure);
    expect(runner.mock.calls.map(([gate]) => gate)).toEqual(["preflight", "lint", "typecheck"]);
  });
});
