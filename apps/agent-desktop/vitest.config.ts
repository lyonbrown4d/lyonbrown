import react from "@vitejs/plugin-react";
import tsconfigPaths from "vite-tsconfig-paths";
import { defineConfig } from "vitest/config";
export default defineConfig({
  plugins: [react(), tsconfigPaths()],
  test: {
    environment: "jsdom",
    include: ["tests/**/*.test.ts", "tests/**/*.test.tsx"],
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts", "src/**/*.tsx"],
      exclude: [
        "src/main/index.ts",
        "src/main/production.ts",
        "src/main/client-worker.ts",
        "src/main/environment.ts",
        "src/main/worker-process.ts",
        "src/preload/index.ts",
        "src/renderer/index.tsx",
        "src/renderer/global.d.ts",
      ],
      thresholds: { lines: 85 },
    },
  },
});
