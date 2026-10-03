import { $, usePowerShell } from "zx";

import { isWorkspaceTask, runWorkspaceTask, type WorkspaceTask } from "./workspace-task-lib.js";
import { configureZxPlatform } from "./zx-platform.js";

configureZxPlatform(process.platform, usePowerShell);

const TASK_TIMEOUT_MS = 15 * 60 * 1000;

async function runRoot(task: WorkspaceTask): Promise<void> {
  switch (task) {
    case "lint":
      await $({
        stdio: "inherit",
        timeout: TASK_TIMEOUT_MS,
      })`eslint scripts/**/*.ts apps/docs/.vitepress/**/*.mts eslint.config.mjs`;
      return;
    case "typecheck":
      await $({ stdio: "inherit", timeout: TASK_TIMEOUT_MS })`tsc --noEmit -p tsconfig.json`;
      return;
    case "test":
      await $({ stdio: "inherit", timeout: TASK_TIMEOUT_MS })`vitest run scripts/tests`;
      return;
    case "build":
      await $({ stdio: "inherit", timeout: TASK_TIMEOUT_MS })`tsc -p tsconfig.build.json`;
  }
}

async function runWorkspace(task: WorkspaceTask): Promise<void> {
  await $({
    stdio: "inherit",
    timeout: TASK_TIMEOUT_MS,
  })`pnpm --recursive --if-present run ${task}`;
}

const requestedTask = process.argv[2] ?? "";
if (!isWorkspaceTask(requestedTask)) {
  console.error(`Usage: workspace-task <lint|typecheck|test|build>; received: ${requestedTask}`);
  process.exitCode = 2;
} else {
  try {
    await runWorkspaceTask(requestedTask, runRoot, runWorkspace);
  } catch (error: unknown) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
