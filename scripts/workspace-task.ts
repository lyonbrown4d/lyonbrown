import { $, usePowerShell } from "zx";

import {
  isWorkspaceTask,
  ROOT_TASK_COMMANDS,
  runWorkspaceTask,
  type WorkspaceTask,
} from "./workspace-task-lib.js";
import { configureZxPlatform } from "./zx-platform.js";

configureZxPlatform(process.platform, usePowerShell);

const TASK_TIMEOUT_MS = 15 * 60 * 1000;

async function runRoot(task: WorkspaceTask): Promise<void> {
  const [executable, ...arguments_] = ROOT_TASK_COMMANDS[task];
  await $({ stdio: "inherit", timeout: TASK_TIMEOUT_MS })`${executable} ${arguments_}`;
}

async function runWorkspace(task: WorkspaceTask): Promise<void> {
  await $({
    stdio: "inherit",
    timeout: TASK_TIMEOUT_MS,
  })`pnpm --recursive run ${task}`;
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
