import tsconfigPaths from "vite-tsconfig-paths";
import { defineConfig } from "vitest/config";

interface TestPlan {
  readonly coverageInclude: string[];
  readonly include: string[];
  readonly exclude: string[];
}

export function testPlan(platform: NodeJS.Platform): TestPlan {
  if (platform === "win32") {
    return {
      coverageInclude: ["src/**/*.ts"],
      include: ["tests/**/*.test.ts"],
      exclude: ["tests/posix*.test.ts", "src/posix/**"],
    };
  }
  return {
    coverageInclude: [
      "src/index.ts",
      "src/platform.ts",
      "src/posix-provider.ts",
      "src/posix/**/*.ts",
      "src/result.ts",
    ],
    include: [
      "tests/package-policy.test.ts",
      "tests/portable-contracts.test.ts",
      "tests/provider.test.ts",
      "tests/model-storage-contracts.test.ts",
      "tests/posix*.test.ts",
    ],
    exclude: [],
  };
}

const plan = testPlan(process.platform);

export default defineConfig({
  plugins: [tsconfigPaths()],
  test: {
    environment: "node",
    include: plan.include,
    exclude: plan.exclude,
    coverage: {
      provider: "v8",
      include: plan.coverageInclude,
      exclude: plan.exclude,
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
