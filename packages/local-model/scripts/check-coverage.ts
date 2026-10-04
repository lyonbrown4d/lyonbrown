import { readFile } from "node:fs/promises";

interface CoverageMetric {
  pct: number;
}

interface CoverageSummary {
  total: Record<"branches" | "functions" | "lines" | "statements", CoverageMetric>;
}

const minimum = 85;
const summary = JSON.parse(
  await readFile(new URL("../coverage/coverage-summary.json", import.meta.url), "utf8"),
) as CoverageSummary;

for (const [name, metric] of Object.entries(summary.total)) {
  if (metric.pct < minimum) {
    throw new Error(`${name} coverage ${String(metric.pct)}% is below ${String(minimum)}%`);
  }
}
