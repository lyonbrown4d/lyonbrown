import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

export default defineConfig({
  resolve: {
    alias: {
      "@lyonbrown/local-control-plane": fileURLToPath(
        new URL("../../packages/local-control-plane/src/index.ts", import.meta.url),
      ),
      "@lyonbrown/protocol": fileURLToPath(
        new URL("../../packages/protocol/src/index.ts", import.meta.url),
      ),
    },
  },
  test: {
    environment: "node",
    include: ["node-tests/**/*.test.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "json-summary"],
      include: ["node-src/**/*.ts"],
      exclude: ["node-src/main.ts"],
      thresholds: {
        lines: 85,
        functions: 85,
        branches: 80,
        statements: 85,
      },
    },
  },
});
