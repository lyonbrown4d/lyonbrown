import { mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { mkdtemp, rm } from "node:fs/promises";

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

configureZxPlatform(process.platform, usePowerShell);

const runPnpm: PnpmRunner = async (cwd, args) => {
  await $({ cwd, quiet: true, timeout: TEST_COMMAND_TIMEOUT_MS })`pnpm ${args}`;
};

async function makeRepository(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "lyonbrown-preflight-"));
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
  await writeFile(join(root, "apps/docs/package.json"), '{"private":true}', "utf8");
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
  test("accepts the required metadata and workspace boundaries", async () => {
    const root = await makeRepository();
    await expect(inspectRepository(root, runPnpm)).resolves.toEqual([]);
  });

  test("uses frozen lockfile-only validation with lifecycle scripts disabled", async () => {
    const root = await makeRepository();
    const calls: { cwd: string; args: readonly string[] }[] = [];

    await inspectRepository(root, (cwd, args) => {
      calls.push({ cwd, args });
      return Promise.resolve();
    });

    expect(calls).toEqual([{ cwd: root, args: FROZEN_LOCKFILE_ARGS }]);
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
