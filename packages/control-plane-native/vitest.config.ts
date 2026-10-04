import { defineConfig } from "vitest/config";

interface TestPlan {
  readonly coverageInclude: string[];
  readonly include: string[];
}

export function testPlan(platform: NodeJS.Platform): TestPlan {
  if (platform === "win32") {
    return { coverageInclude: ["src/**/*.ts"], include: ["tests/**/*.test.ts"] };
  }
  return {
    coverageInclude: ["src/index.ts", "src/platform.ts", "src/posix-provider.ts", "src/result.ts"],
    include: [
      "tests/package-policy.test.ts",
      "tests/portable-contracts.test.ts",
      "tests/provider.test.ts",
    ],
  };
}

const plan = testPlan(process.platform);

export default defineConfig({
  test: {
    environment: "node",
    include: plan.include,
    coverage: {
      provider: "v8",
      include: plan.coverageInclude,
      reporter: ["text", "json-summary"],
      thresholds: {
        branches: 85,
        functions: 85,
        lines: 85,
        statements: 85,
      },
    },
  },
});
