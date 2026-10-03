import { describe, expect, test, vi } from "vitest";

import { ROOT_TASK_COMMANDS, runWorkspaceTask } from "../workspace-task-lib.js";

describe("workspace task orchestration", () => {
  test("scopes root tests to the canonical scripts test directory", () => {
    expect(ROOT_TASK_COMMANDS.test).toEqual(["vitest", "run", "--dir", "scripts/tests"]);
  });

  test("validates the root before running workspace packages", async () => {
    const calls: string[] = [];

    await runWorkspaceTask(
      "lint",
      (task) => {
        calls.push(`root:${task}`);
        return Promise.resolve();
      },
      (task) => {
        calls.push(`workspace:${task}`);
        return Promise.resolve();
      },
    );

    expect(calls).toEqual(["root:lint", "workspace:lint"]);
  });

  test("does not run workspace packages after a root failure", async () => {
    const failure = new Error("root failed");
    const workspaceRunner = vi.fn();

    await expect(
      runWorkspaceTask(
        "build",
        () => {
          throw failure;
        },
        workspaceRunner,
      ),
    ).rejects.toBe(failure);
    expect(workspaceRunner).not.toHaveBeenCalled();
  });
});
