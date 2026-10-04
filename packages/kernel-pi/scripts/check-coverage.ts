import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

interface CoverageMetric {
  readonly pct: number | "Unknown";
}

interface CoverageSummary {
  readonly lines: CoverageMetric;
}

const minimumLineCoverage = 85;
const reportPath = resolve("coverage/coverage-summary.json");
const parsed: unknown = JSON.parse(await readFile(reportPath, "utf8"));
if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
  throw new TypeError("Coverage summary must be an object");
}

const failures: string[] = [];
for (const [file, summary] of Object.entries(parsed)) {
  if (file === "total") continue;
  if (!isCoverageSummary(summary)) throw new TypeError(`Invalid coverage summary for ${file}`);
  const percentage = summary.lines.pct;
  if (percentage === "Unknown" || percentage < minimumLineCoverage) {
    failures.push(`${file}: ${String(percentage)}%`);
  }
}

if (failures.length > 0) {
  throw new Error(
    `Per-file line coverage must be at least ${String(minimumLineCoverage)}%:\n${failures.join("\n")}`,
  );
}

function isCoverageSummary(value: unknown): value is CoverageSummary {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const lines = Object.getOwnPropertyDescriptor(value, "lines");
  if (!lines || !("value" in lines) || typeof lines.value !== "object" || lines.value === null) {
    return false;
  }
  const percentage = Object.getOwnPropertyDescriptor(lines.value, "pct");
  return (
    percentage !== undefined &&
    "value" in percentage &&
    (typeof percentage.value === "number" || percentage.value === "Unknown")
  );
}
