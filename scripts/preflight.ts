import { $, usePowerShell } from "zx";

import {
  NODE_VERSION,
  PACKAGE_MANAGER,
  assertNodeVersion,
  assertPnpmVersion,
  inspectRepository,
  type PnpmRunner,
} from "@lyonbrown/workspace/internal/preflight-lib";
import { configureZxPlatform } from "@lyonbrown/workspace/internal/zx-platform";

configureZxPlatform(process.platform, usePowerShell);

const LOCKFILE_CHECK_TIMEOUT_MS = 60_000;

const runPnpm: PnpmRunner = async (cwd, args) => {
  await $({ cwd, quiet: true, timeout: LOCKFILE_CHECK_TIMEOUT_MS })`pnpm ${args}`;
};

async function main(): Promise<void> {
  assertNodeVersion(process.version);
  const pnpmVersion = (await $({ quiet: true })`pnpm --version`).stdout.trim();
  assertPnpmVersion(pnpmVersion);

  const failures = await inspectRepository(process.cwd(), runPnpm);
  if (failures.length > 0) {
    throw new Error(`Preflight failed:\n- ${failures.join("\n- ")}`);
  }

  console.log(`Preflight passed: Node ${process.version} (minimum ${NODE_VERSION})`);
  console.log(`Preflight passed: ${PACKAGE_MANAGER}`);
}

try {
  await main();
} catch (error: unknown) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
