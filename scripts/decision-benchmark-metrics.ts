import type { RuntimePreKernelDecision } from "@lyonbrown/agent-runtime";

import type { DecisionBenchmarkFixture } from "@lyonbrown/workspace/internal/decision-benchmark-fixtures";

export interface DecisionBenchmarkResult {
  readonly fixture: DecisionBenchmarkFixture;
  readonly latencyMs: number;
  readonly outcome: RuntimePreKernelDecision;
}

const AXES = ["intent", "risk", "requiresTools"] as const;
type Axis = (typeof AXES)[number];

export function summarizeDecisionBenchmark(results: readonly DecisionBenchmarkResult[]) {
  if (results.length === 0) throw new Error("empty_decision_benchmark");
  const fallbackCount = results.filter(({ outcome }) => outcome.kind === "fallback").length;
  return {
    samples: results.length,
    fallbackCount,
    fallbackRate: fallbackCount / results.length,
    axes: Object.fromEntries(AXES.map((axis) => [axis, summarizeAxis(results, axis)])),
    byLocale: Object.fromEntries(
      (["en", "zh"] as const).map((locale) => {
        const subset = results.filter(({ fixture }) => fixture.locale === locale);
        return [
          locale,
          {
            samples: subset.length,
            axes: Object.fromEntries(AXES.map((axis) => [axis, summarizeAxis(subset, axis)])),
          },
        ];
      }),
    ),
    coldMs: results[0]?.latencyMs,
    warmP50Ms: percentile(
      results.slice(1).map(({ latencyMs }) => latencyMs),
      0.5,
    ),
    warmP95Ms: percentile(
      results.slice(1).map(({ latencyMs }) => latencyMs),
      0.95,
    ),
  };
}

function summarizeAxis(results: readonly DecisionBenchmarkResult[], axis: Axis) {
  let correct = 0;
  let brierSum = 0;
  let decided = 0;
  const bins = Array.from({ length: 10 }, () => ({ count: 0, correct: 0, confidence: 0 }));
  for (const { fixture, outcome } of results) {
    if (outcome.kind !== "decided") continue;
    decided += 1;
    const matched = outcome[axis] === fixture.expected[axis];
    if (matched) correct += 1;
    const probabilities = probabilitiesFor(outcome.probabilities, axis);
    const confidence = probabilities[String(outcome[axis])];
    if (
      confidence === undefined ||
      !Number.isFinite(confidence) ||
      confidence < 0 ||
      confidence > 1
    )
      throw new Error("invalid_benchmark_confidence");
    const bin = bins[Math.min(9, Math.floor(confidence * 10))];
    if (bin === undefined) throw new Error("invalid_benchmark_bin");
    bin.count += 1;
    bin.correct += matched ? 1 : 0;
    bin.confidence += confidence;
    brierSum += Object.entries(probabilities).reduce(
      (sum, [label, probability]) =>
        sum + (probability - (label === String(fixture.expected[axis]) ? 1 : 0)) ** 2,
      0,
    );
  }
  return {
    correct,
    accuracy: results.length === 0 ? null : correct / results.length,
    decidedCount: decided,
    // ECE and Brier are conditional on a validated result; fallback rate is reported separately.
    ece:
      decided === 0
        ? null
        : bins.reduce(
            (sum, bin) =>
              bin.count === 0
                ? sum
                : sum +
                  (Math.abs(bin.correct / bin.count - bin.confidence / bin.count) * bin.count) /
                    decided,
            0,
          ),
    brier: decided === 0 ? null : brierSum / decided,
  };
}

function percentile(values: readonly number[], quantile: number): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * quantile) - 1)] ?? null;
}

function probabilitiesFor(value: unknown, axis: Axis): Readonly<Record<string, number>> {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw new Error("missing_benchmark_probabilities");
  const distribution = (value as Record<string, unknown>)[axis];
  if (typeof distribution !== "object" || distribution === null || Array.isArray(distribution))
    throw new Error("missing_benchmark_probabilities");
  const record = distribution as Record<string, unknown>;
  const labels =
    axis === "intent"
      ? ["chat", "code-change", "system-operation"]
      : axis === "risk"
        ? ["low", "medium", "high"]
        : ["false", "true"];
  if (Object.keys(record).length !== labels.length)
    throw new Error("invalid_benchmark_probabilities");
  const entries = labels.map((label) => {
    const probability = record[label];
    if (
      typeof probability !== "number" ||
      !Number.isFinite(probability) ||
      probability < 0 ||
      probability > 1
    )
      throw new Error("invalid_benchmark_probabilities");
    return [label, probability] as const;
  });
  if (Math.abs(entries.reduce((sum, [, probability]) => sum + probability, 0) - 1) > 0.000001)
    throw new Error("invalid_benchmark_probabilities");
  return Object.fromEntries(entries);
}
