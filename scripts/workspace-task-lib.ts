export const WORKSPACE_TASKS = ["lint", "typecheck", "test", "build"] as const;

export type WorkspaceTask = (typeof WORKSPACE_TASKS)[number];
export type TaskRunner = (task: WorkspaceTask) => Promise<void>;

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
