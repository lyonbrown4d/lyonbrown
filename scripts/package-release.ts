import { constants } from "node:fs";
import { access, copyFile, lstat, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";

import { $, usePowerShell } from "zx";

import {
  resolveReleaseTarget,
  windowsAgentLauncher,
} from "@lyonbrown/workspace/internal/release-artifact-lib";
import { configureZxPlatform } from "@lyonbrown/workspace/internal/zx-platform";

configureZxPlatform(process.platform, usePowerShell);

interface ProductManifest {
  readonly name?: unknown;
  readonly version?: unknown;
}

async function assertMissing(path: string): Promise<void> {
  try {
    await lstat(path);
  } catch (error: unknown) {
    if (isNodeError(error) && error.code === "ENOENT") return;
    throw error;
  }
  throw new Error(`Release output already exists: ${path}`);
}

function isNodeError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error;
}

async function main(): Promise<void> {
  const workspaceRoot = process.cwd();
  const target = resolveReleaseTarget(workspaceRoot, process.argv.slice(2));
  await assertMissing(target.outputDirectory);
  await mkdir(dirname(target.outputDirectory), { recursive: true });

  await $({ cwd: workspaceRoot, stdio: "inherit" })`pnpm --filter @lyonbrown/agent-cli build`;
  await $({
    cwd: workspaceRoot,
    stdio: "inherit",
  })`pnpm --config.node-linker=hoisted --filter @lyonbrown/agent-cli deploy --prod --legacy ${target.outputDirectory}`;

  const runtimeDirectory = join(target.outputDirectory, "runtime");
  await mkdir(runtimeDirectory);
  await copyFile(process.execPath, join(runtimeDirectory, "node.exe"), constants.COPYFILE_EXCL);
  await writeFile(join(target.outputDirectory, "agent.cmd"), windowsAgentLauncher(), {
    encoding: "utf8",
    flag: "wx",
  });

  const product = JSON.parse(
    await readFile(join(target.outputDirectory, "package.json"), "utf8"),
  ) as ProductManifest;
  if (product.name !== "@lyonbrown/agent-cli" || typeof product.version !== "string") {
    throw new Error("Deployed product manifest is invalid");
  }

  await access(join(target.outputDirectory, "dist", "main.js"), constants.R_OK);
  await access(
    join(target.outputDirectory, "node_modules", "@lyonbrown", "agentd"),
    constants.R_OK,
  );
  await $({
    cwd: workspaceRoot,
    stdio: "inherit",
  })`pnpm tsx scripts/smoke-release-artifact.ts --artifact ${target.outputDirectory}`;
  console.log(`Release artifact ready: ${target.outputDirectory}`);
}

try {
  await main();
} catch (error: unknown) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
