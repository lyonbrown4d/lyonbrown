import { access, readFile, readdir } from "node:fs/promises";
import { join } from "node:path";

import { describe, expect, test } from "vitest";

const ROOT = process.cwd();

interface RootPackageJson {
  readonly scripts?: Readonly<Record<string, string>>;
  readonly devDependencies?: Readonly<Record<string, string>>;
}

describe("moon workspace orchestration", () => {
  test("routes the root development and quality workflows through pinned moon", async () => {
    const packageJson = JSON.parse(
      await readFile(join(ROOT, "package.json"), "utf8"),
    ) as RootPackageJson;

    expect(packageJson.devDependencies?.["@moonrepo/cli"]).toBe("2.6.0");
    expect(packageJson.scripts).toMatchObject({
      dev: "moon run workspace:dev",
      "dev:docs": "moon run docs:dev",
      "dependencies:build": "moon run agentd:build agent-launch:build",
      lint: "moon run :lint",
      typecheck: "moon run :typecheck",
      test: "moon run :test --concurrency 1",
      build: "moon run :build",
    });
  });

  test("maps every pnpm application and package into the moon project graph", async () => {
    const workspaceConfig = await readFile(join(ROOT, ".moon", "workspace.yml"), "utf8");
    const workspacePackages = await packageDirectories("apps", "packages");

    for (const directory of workspacePackages) {
      expect(workspaceConfig).toContain(`    ${directory}: ".moon/projects/${directory}"`);
      await expect(access(join(ROOT, ".moon", "projects", directory, "moon.yml"))).resolves.toBe(
        undefined,
      );
    }
  });

  test("removes the superseded custom workspace task runner", async () => {
    const obsoleteFiles = [
      "scripts/workspace-task.ts",
      "scripts/workspace-task-lib.ts",
      "scripts/tests/workspace-task.test.ts",
    ];

    for (const obsoleteFile of obsoleteFiles) {
      await expect(access(join(ROOT, obsoleteFile))).rejects.toMatchObject({ code: "ENOENT" });
    }
  });
});

async function packageDirectories(...roots: readonly string[]): Promise<readonly string[]> {
  const directories = await Promise.all(
    roots.map(async (root) => {
      const entries = await readdir(join(ROOT, root), { withFileTypes: true });
      return entries.filter((entry) => entry.isDirectory()).map((entry) => entry.name);
    }),
  );
  return directories.flat().sort();
}
