import type { RuntimePreKernelDecision } from "@lyonbrown/agent-runtime";
import { describe, expect, it } from "vitest";

import { DECISION_BENCHMARK_FIXTURES } from "@lyonbrown/workspace/internal/decision-benchmark-fixtures";
import {
  summarizeDecisionBenchmark,
  type DecisionBenchmarkResult,
} from "@lyonbrown/workspace/internal/decision-benchmark-metrics";

const fixture = DECISION_BENCHMARK_FIXTURES[0];
if (fixture === undefined) throw new Error("benchmark fixture missing");
const PROBABILITIES = {
  intent: { chat: 1, "code-change": 0, "system-operation": 0 },
  risk: { low: 1, medium: 0, high: 0 },
  requiresTools: { false: 1, true: 0 },
};
const decided: RuntimePreKernelDecision = {
  kind: "decided",
  questionVersion: "system-one-v1",
  providerId: "test",
  modelId: "test",
  modelRevision: "test",
  intent: "chat",
  risk: "low",
  requiresTools: false,
  confidence: 1,
  probabilities: {
    intent: { chat: 1, "code-change": 0, "system-operation": 0 },
    risk: { low: 1, medium: 0, high: 0 },
    requiresTools: { false: 1, true: 0 },
  },
  action: "continue_pi",
};

describe("decision benchmark evidence", () => {
  it("keeps bilingual paired fixtures unique and balanced by intent", () => {
    expect(DECISION_BENCHMARK_FIXTURES).toHaveLength(36);
    expect(new Set(DECISION_BENCHMARK_FIXTURES.map(({ id }) => id)).size).toBe(36);
    expect(DECISION_BENCHMARK_FIXTURES.filter(({ locale }) => locale === "zh")).toHaveLength(18);
    for (const intent of ["chat", "code-change", "system-operation"])
      expect(
        DECISION_BENCHMARK_FIXTURES.filter(({ expected }) => expected.intent === intent),
      ).toHaveLength(12);
  });

  it("includes fallbacks in accuracy and reports conditional calibration separately", () => {
    const results: DecisionBenchmarkResult[] = [
      { fixture, latencyMs: 10, outcome: decided },
      {
        fixture,
        latencyMs: 20,
        outcome: {
          ...decided,
          intent: "code-change",
          probabilities: {
            ...PROBABILITIES,
            intent: { chat: 0.25, "code-change": 0.75, "system-operation": 0 },
          },
        },
      },
      {
        fixture: { ...fixture, locale: "zh" },
        latencyMs: 30,
        outcome: {
          kind: "fallback",
          reason: "provider_unavailable",
          questionVersion: "system-one-v1",
          action: "continue_pi",
        },
      },
    ];
    const summary = summarizeDecisionBenchmark(results);
    expect(summary.fallbackRate).toBeCloseTo(1 / 3);
    expect(summary.axes.intent?.accuracy).toBeCloseTo(1 / 3);
    expect(summary.axes.intent?.ece).toBeCloseTo(0.375);
    expect(summary.axes.intent?.brier).toBeCloseTo(0.5625);
    expect(summary.byLocale.zh?.axes.intent?.ece).toBeNull();
    expect(summary.coldMs).toBe(10);
    expect(summary.warmP50Ms).toBe(20);
    expect(summary.warmP95Ms).toBe(30);
  });

  it("rejects empty runs and missing probability evidence", () => {
    expect(() => summarizeDecisionBenchmark([])).toThrow("empty_decision_benchmark");
    expect(() =>
      summarizeDecisionBenchmark([
        { fixture, latencyMs: 1, outcome: { ...decided, probabilities: {} } },
      ]),
    ).toThrow("missing_benchmark_probabilities");
  });
});
