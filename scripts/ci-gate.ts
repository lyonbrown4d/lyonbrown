import { $, usePowerShell } from "zx";

import { runCiGate } from "./ci-gate-lib.ts";
import { configureZxPlatform } from "./zx-platform.ts";

configureZxPlatform(process.platform, usePowerShell);

const GATE_TIMEOUT_MS = 15 * 60 * 1000;

try {
  await runCiGate(async (gate) => {
    const startedAt = performance.now();
    console.log(`\n[ci-gate] starting ${gate}`);
    await $({ stdio: "inherit", timeout: GATE_TIMEOUT_MS })`pnpm run ${gate}`;
    const elapsedMs = Math.round(performance.now() - startedAt);
    console.log(`[ci-gate] ${gate} passed in ${String(elapsedMs)} ms`);
  });
} catch (error: unknown) {
  console.error("[ci-gate] stopped on the first failing gate");
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
