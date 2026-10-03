import { access, mkdir, mkdtemp, readFile, rm, stat, utimes, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { isAbsolute, join, relative } from "node:path";

import { afterEach, describe, expect, test } from "vitest";
import { $, usePowerShell } from "zx";

import {
  FROZEN_LOCKFILE_ARGS,
  REQUIRED_FILES,
  assertNodeVersion,
  assertPnpmVersion,
  inspectRepository,
  type PnpmRunner,
} from "../preflight-lib.js";
import { configureZxPlatform } from "../zx-platform.js";

const temporaryDirectories: string[] = [];
const TEST_COMMAND_TIMEOUT_MS = 30_000;
const REQUIRED_WORKSPACE_SCRIPTS = ["lint", "typecheck", "test", "build"] as const;

configureZxPlatform(process.platform, usePowerShell);

const runPnpm: PnpmRunner = async (cwd, args) => {
  await $({ cwd, quiet: true, timeout: TEST_COMMAND_TIMEOUT_MS })`pnpm ${args}`;
};

function workspacePackage(name: string): string {
  return JSON.stringify({
    name,
    private: true,
    scripts: Object.fromEntries(REQUIRED_WORKSPACE_SCRIPTS.map((script) => [script, "test"])),
  });
}

async function makeRepository(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "lyonbrown preflight source "));
  temporaryDirectories.push(root);

  await Promise.all(
    REQUIRED_FILES.map(async (relativePath) => {
      const path = join(root, relativePath);
      await mkdir(join(path, ".."), { recursive: true });
      await writeFile(path, "\n", "utf8");
    }),
  );
  await writeFile(
    join(root, "package.json"),
    JSON.stringify({
      packageManager: "pnpm@10.33.4",
      engines: { node: ">=24.15.0 <25" },
      scripts: Object.fromEntries(
        [
          "preflight",
          "lint",
          "typecheck",
          "test",
          "build",
          "ci-gate",
          "format",
          "format:check",
          "scripts:verify",
          "audit",
        ].map((name) => [name, "test"]),
      ),
    }),
    "utf8",
  );
  await writeFile(join(root, "apps/docs/package.json"), workspacePackage("@fixture/docs"), "utf8");
  await mkdir(join(root, "packages/example"), { recursive: true });
  await mkdir(join(root, "tools/example"), { recursive: true });
  await writeFile(
    join(root, "packages/example/package.json"),
    workspacePackage("@fixture/package"),
    "utf8",
  );
  await writeFile(
    join(root, "tools/example/package.json"),
    workspacePackage("@fixture/tool"),
    "utf8",
  );
  await writeFile(join(root, ".npmrc"), "strict-peer-dependencies=false\n", "utf8");
  await writeFile(
    join(root, "pnpm-workspace.yaml"),
    "packages:\n  - apps/*\n  - packages/*\n  - tools/*\n",
    "utf8",
  );
  await writeFile(
    join(root, "pnpm-lock.yaml"),
    [
      "lockfileVersion: '9.0'",
      "settings:",
      "  autoInstallPeers: true",
      "  excludeLinksFromLockfile: false",
      "importers:",
      "  .: {}",
      "  apps/docs: {}",
      "  packages/example: {}",
      "  tools/example: {}",
      "",
    ].join("\n"),
    "utf8",
  );
  return root;
}

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { recursive: true })));
});

describe("toolchain version checks", () => {
  test("accepts the pinned baseline", () => {
    expect(() => {
      assertNodeVersion("v24.15.0");
    }).not.toThrow();
    expect(() => {
      assertPnpmVersion("10.33.4");
    }).not.toThrow();
  });

  test("rejects older Node and mismatched pnpm versions", () => {
    expect(() => {
      assertNodeVersion("v24.14.9");
    }).toThrow(/Node 24\.15\.0 or newer/);
    expect(() => {
      assertNodeVersion("v25.0.0");
    }).toThrow(/Node 24\.x/);
    expect(() => {
      assertPnpmVersion("10.33.3");
    }).toThrow(/pnpm 10\.33\.4/);
  });
});

describe("repository checks", () => {
  test("accepts complete workspace task contracts", async () => {
    const root = await makeRepository();
    await expect(inspectRepository(root, runPnpm)).resolves.toEqual([]);
  });

  test.each(REQUIRED_WORKSPACE_SCRIPTS)(
    "rejects a workspace package missing its %s script",
    async (missingScript) => {
      const root = await makeRepository();
      const scripts = Object.fromEntries(
        REQUIRED_WORKSPACE_SCRIPTS.filter((script) => script !== missingScript).map((script) => [
          script,
          "test",
        ]),
      );
      await writeFile(
        join(root, "apps/docs/package.json"),
        JSON.stringify({ name: "@fixture/docs", private: true, scripts }),
        "utf8",
      );

      await expect(inspectRepository(root, runPnpm)).resolves.toContain(
        `Workspace package @fixture/docs (apps/docs/package.json) is missing scripts: ${missingScript}`,
      );
    },
  );

  test("uses frozen lockfile-only validation with lifecycle scripts disabled", async () => {
    const root = await makeRepository();
    const calls: { cwd: string; args: readonly string[] }[] = [];
    let mirrorRoot = "";

    await inspectRepository(root, async (cwd, args) => {
      mirrorRoot = cwd;
      calls.push({ cwd, args });
      await expect(access(join(cwd, "package.json"))).resolves.toBeUndefined();
      await expect(access(join(cwd, "pnpm-workspace.yaml"))).resolves.toBeUndefined();
      await expect(access(join(cwd, "pnpm-lock.yaml"))).resolves.toBeUndefined();
      await expect(access(join(cwd, "apps/docs/package.json"))).resolves.toBeUndefined();
      await expect(access(join(cwd, "packages/example/package.json"))).resolves.toBeUndefined();
      await expect(access(join(cwd, "tools/example/package.json"))).resolves.toBeUndefined();
      await expect(access(join(cwd, ".npmrc"))).resolves.toBeUndefined();
      await expect(access(join(cwd, "node_modules"))).rejects.toBeDefined();
    });

    expect(calls).toEqual([{ cwd: mirrorRoot, args: FROZEN_LOCKFILE_ARGS }]);
    expect(mirrorRoot).not.toBe(root);
    const relativeMirrorPath = relative(tmpdir(), mirrorRoot);
    expect(relativeMirrorPath).not.toMatch(/^\.\./);
    expect(isAbsolute(relativeMirrorPath)).toBe(false);
    await expect(access(mirrorRoot)).rejects.toBeDefined();
  });

  test("does not change source lockfile contents or modification time", async () => {
    const root = await makeRepository();
    const lockfilePath = join(root, "pnpm-lock.yaml");
    const fixedTimestamp = new Date("2020-01-02T03:04:05.000Z");
    await utimes(lockfilePath, fixedTimestamp, fixedTimestamp);
    const contentsBefore = await readFile(lockfilePath, "utf8");
    const modifiedBefore = (await stat(lockfilePath, { bigint: true })).mtimeNs;

    await expect(inspectRepository(root, runPnpm)).resolves.toEqual([]);

    await expect(readFile(lockfilePath, "utf8")).resolves.toBe(contentsBefore);
    expect((await stat(lockfilePath, { bigint: true })).mtimeNs).toBe(modifiedBefore);
  });

  test("removes the manifest mirror when pnpm validation fails", async () => {
    const root = await makeRepository();
    let mirrorRoot = "";

    const failures = await inspectRepository(root, (cwd) => {
      mirrorRoot = cwd;
      return Promise.reject(new Error("synthetic frozen install failure"));
    });

    expect(failures).toEqual(
      expect.arrayContaining([expect.stringMatching(/synthetic frozen install failure/)]),
    );
    await expect(access(mirrorRoot)).rejects.toBeDefined();
  });

  test("reports missing files and workspace boundary drift", async () => {
    const root = await makeRepository();
    await rm(join(root, "pnpm-lock.yaml"));
    await writeFile(join(root, "pnpm-workspace.yaml"), "packages:\n  - '**'\n", "utf8");

    await expect(inspectRepository(root, runPnpm)).resolves.toEqual(
      expect.arrayContaining([
        expect.stringMatching(/pnpm-lock\.yaml/),
        expect.stringMatching(/workspace package globs/),
      ]),
    );
  });

  test("rejects an empty lockfile", async () => {
    const root = await makeRepository();
    await writeFile(join(root, "pnpm-lock.yaml"), "", "utf8");

    await expect(inspectRepository(root, runPnpm)).resolves.toEqual(
      expect.arrayContaining([expect.stringMatching(/frozen lockfile validation.*empty/i)]),
    );
  });

  test("rejects a malformed lockfile", async () => {
    const root = await makeRepository();
    await writeFile(join(root, "pnpm-lock.yaml"), "lockfileVersion: [invalid", "utf8");

    await expect(inspectRepository(root, runPnpm)).resolves.toEqual(
      expect.arrayContaining([expect.stringMatching(/frozen lockfile validation/i)]),
    );
  });

  test("rejects manifest dependency drift", async () => {
    const root = await makeRepository();
    await writeFile(
      join(root, "apps/docs/package.json"),
      JSON.stringify({ private: true, devDependencies: { vitepress: "1.6.4" } }),
      "utf8",
    );

    await expect(inspectRepository(root, runPnpm)).resolves.toEqual(
      expect.arrayContaining([expect.stringMatching(/frozen lockfile validation/i)]),
    );
  });
});
