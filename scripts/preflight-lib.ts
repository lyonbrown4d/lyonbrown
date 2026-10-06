import { access, readFile } from "node:fs/promises";
import { join } from "node:path";

import {
  inspectWorkspaceManifests,
  validateFrozenLockfile,
  type PnpmRunner,
} from "@lyonbrown/workspace/internal/preflight-workspace";

export {
  FROZEN_LOCKFILE_ARGS,
  type PnpmRunner,
} from "@lyonbrown/workspace/internal/preflight-workspace";

export const NODE_VERSION = "24.15.0";
export const PNPM_VERSION = "10.33.4";
export const PACKAGE_MANAGER = `pnpm@${PNPM_VERSION}`;
export const REQUIRED_FILES = [
  "package.json",
  "pnpm-lock.yaml",
  "pnpm-workspace.yaml",
  "tsconfig.json",
  "tsconfig.build.json",
  "eslint.config.mjs",
  "AGENTS.md",
  "apps/docs/package.json",
] as const;

const REQUIRED_SCRIPTS = [
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
] as const;

const WORKSPACE_PACKAGES = ["apps/*", "packages/*", "tools/*"] as const;

interface PackageMetadata {
  packageManager?: unknown;
  engines?: { node?: unknown };
  scripts?: Record<string, unknown>;
}

function parseVersion(version: string): readonly [number, number, number] {
  const match = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(version.trim());
  if (match === null) {
    throw new Error(`Invalid semantic version: ${version}`);
  }
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

export function assertNodeVersion(version: string): void {
  const [major, minor, patch] = parseVersion(version);
  if (major !== 24) {
    throw new Error(`Node 24.x is required; found ${version}`);
  }
  if (minor < 15 || (minor === 15 && patch < 0)) {
    throw new Error(`Node 24.15.0 or newer is required; found ${version}`);
  }
}

export function assertPnpmVersion(version: string): void {
  if (version.trim() !== PNPM_VERSION) {
    throw new Error(`pnpm ${PNPM_VERSION} is required; found ${version.trim()}`);
  }
}

function parseWorkspacePackages(contents: string): string[] {
  const lines = contents.split(/\r?\n/);
  const packagesIndex = lines.findIndex((line) => /^packages:\s*$/.test(line));
  if (packagesIndex < 0) {
    return [];
  }

  const patterns: string[] = [];
  for (const line of lines.slice(packagesIndex + 1)) {
    const match = /^\s{2}-\s+['"]?([^'"]+)['"]?\s*$/.exec(line);
    if (match?.[1] !== undefined) {
      patterns.push(match[1]);
      continue;
    }
    if (line.trim() !== "" && !/^\s/.test(line)) {
      break;
    }
  }
  return patterns;
}

function equalValues(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

export async function inspectRepository(root: string, runPnpm: PnpmRunner): Promise<string[]> {
  const failures: string[] = [];
  await Promise.all(
    REQUIRED_FILES.map(async (relativePath) => {
      try {
        await access(join(root, relativePath));
      } catch {
        failures.push(`Missing required file: ${relativePath}`);
      }
    }),
  );

  try {
    const packageJson = JSON.parse(
      await readFile(join(root, "package.json"), "utf8"),
    ) as PackageMetadata;
    if (packageJson.packageManager !== PACKAGE_MANAGER) {
      failures.push(`packageManager must be ${PACKAGE_MANAGER}`);
    }
    if (packageJson.engines?.node !== ">=24.15.0 <25") {
      failures.push("engines.node must be >=24.15.0 <25");
    }
    for (const script of REQUIRED_SCRIPTS) {
      if (typeof packageJson.scripts?.[script] !== "string") {
        failures.push(`Missing package script: ${script}`);
      }
    }
  } catch (error: unknown) {
    failures.push(`Unable to validate package.json: ${String(error)}`);
  }

  try {
    const workspacePackages = parseWorkspacePackages(
      await readFile(join(root, "pnpm-workspace.yaml"), "utf8"),
    );
    if (!equalValues(workspacePackages, WORKSPACE_PACKAGES)) {
      failures.push(`workspace package globs must be: ${WORKSPACE_PACKAGES.join(", ")}`);
    }
  } catch (error: unknown) {
    failures.push(`Unable to validate pnpm-workspace.yaml: ${String(error)}`);
  }

  let workspaceManifestPaths: string[] = [];
  try {
    const workspaceInspection = await inspectWorkspaceManifests(root);
    workspaceManifestPaths = workspaceInspection.manifestPaths;
    failures.push(...workspaceInspection.failures);
  } catch (error: unknown) {
    failures.push(`Unable to inspect workspace package manifests: ${String(error)}`);
  }

  try {
    await validateFrozenLockfile(root, runPnpm, workspaceManifestPaths);
  } catch (error: unknown) {
    failures.push(`Frozen lockfile validation failed: ${String(error)}`);
  }

  return failures.sort();
}
