export const WORKSPACE_TASKS = ["lint", "typecheck", "test", "build"] as const;

export type WorkspaceTask = (typeof WORKSPACE_TASKS)[number];
export type TaskRunner = (task: WorkspaceTask) => Promise<void>;
export type RootTaskCommand = readonly [executable: string, ...arguments_: string[]];

export const ROOT_TASK_COMMANDS = {
  lint: ["eslint", "scripts/**/*.ts", "apps/docs/.vitepress/**/*.mts", "eslint.config.mjs"],
  typecheck: ["tsc", "--noEmit", "-p", "tsconfig.json"],
  test: ["vitest", "run", "--dir", "scripts/tests"],
  build: ["tsc", "-p", "tsconfig.build.json"],
} as const satisfies Record<WorkspaceTask, RootTaskCommand>;

export function isWorkspaceTask(value: string): value is WorkspaceTask {
  return WORKSPACE_TASKS.some((task) => task === value);
}

export async function runWorkspaceTask(
  task: WorkspaceTask,
  runRoot: TaskRunner,
  runWorkspace: TaskRunner,
): Promise<void> {
  await runRoot(task);
  await runWorkspace(task);
}
