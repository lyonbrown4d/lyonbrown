import { describe, expect, test, vi } from "vitest";

import { runWorkspaceTask } from "../workspace-task-lib.js";

describe("workspace task orchestration", () => {
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
