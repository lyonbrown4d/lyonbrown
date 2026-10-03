import { access, copyFile, mkdir, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from "node:path";

export const FROZEN_LOCKFILE_ARGS = [
  "install",
  "--lockfile-only",
  "--frozen-lockfile",
  "--ignore-scripts",
] as const;

export type PnpmRunner = (cwd: string, args: readonly string[]) => Promise<void>;

const REQUIRED_WORKSPACE_SCRIPTS = ["lint", "typecheck", "test", "build"] as const;
const WORKSPACE_ROOTS = ["apps", "packages", "tools"] as const;
const MIRROR_PREFIX = "lyonbrown preflight mirror ";
const ROOT_MANIFEST_PATHS = ["package.json", "pnpm-workspace.yaml", "pnpm-lock.yaml"] as const;

interface WorkspacePackageMetadata {
  name?: unknown;
  scripts?: Record<string, unknown>;
}

export interface WorkspaceManifestInspection {
  failures: string[];
  manifestPaths: string[];
}

function isNotFoundError(error: unknown): error is NodeJS.ErrnoException {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

function displayPath(relativePath: string): string {
  return relativePath.split(sep).join("/");
}

function resolveWithin(base: string, relativePath: string): string {
  const resolvedBase = resolve(base);
  const resolvedPath = resolve(resolvedBase, relativePath);
  const pathFromBase = relative(resolvedBase, resolvedPath);
  if (pathFromBase === "" || pathFromBase === ".." || pathFromBase.startsWith(`..${sep}`)) {
    throw new Error(`Refusing path outside manifest mirror: ${relativePath}`);
  }
  if (isAbsolute(pathFromBase)) {
    throw new Error(`Refusing absolute path in manifest mirror: ${relativePath}`);
  }
  return resolvedPath;
}

async function findWorkspaceManifests(root: string): Promise<string[]> {
  const manifests: string[] = [];
  for (const workspaceRoot of WORKSPACE_ROOTS) {
    let entries;
    try {
      entries = await readdir(join(root, workspaceRoot), { withFileTypes: true });
    } catch (error: unknown) {
      if (isNotFoundError(error)) {
        continue;
      }
      throw error;
    }

    // Node has no batched stat/access API; only direct workspace child manifests are examined.
    for (const entry of entries) {
      if (!entry.isDirectory()) {
        continue;
      }
      const manifestPath = join(workspaceRoot, entry.name, "package.json");
      try {
        await access(join(root, manifestPath));
        manifests.push(manifestPath);
      } catch (error: unknown) {
        if (!isNotFoundError(error)) {
          throw error;
        }
      }
    }
  }
  return manifests.sort();
}

export async function inspectWorkspaceManifests(
  root: string,
): Promise<WorkspaceManifestInspection> {
  const manifestPaths = await findWorkspaceManifests(root);
  const failures: string[] = [];
  for (const manifestPath of manifestPaths) {
    const path = displayPath(manifestPath);
    try {
      const metadata = JSON.parse(
        await readFile(join(root, manifestPath), "utf8"),
      ) as WorkspacePackageMetadata;
      const packageName =
        typeof metadata.name === "string" && metadata.name.trim() !== ""
          ? metadata.name
          : "<unnamed>";
      const missingScripts = REQUIRED_WORKSPACE_SCRIPTS.filter(
        (script) => typeof metadata.scripts?.[script] !== "string",
      );
      if (missingScripts.length > 0) {
        failures.push(
          `Workspace package ${packageName} (${path}) is missing scripts: ${missingScripts.join(
            ", ",
          )}`,
        );
      }
    } catch (error: unknown) {
      failures.push(`Unable to validate workspace package manifest ${path}: ${String(error)}`);
    }
  }
  return { failures, manifestPaths };
}

async function findOptionalPnpmConfig(root: string): Promise<string[]> {
  try {
    await access(join(root, ".npmrc"));
    return [".npmrc"];
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return [];
    }
    throw error;
  }
}

async function createManifestMirror(
  root: string,
  workspaceManifestPaths: readonly string[],
): Promise<string> {
  const mirrorRoot = await mkdtemp(join(tmpdir(), MIRROR_PREFIX));
  try {
    const manifestPaths = [
      ...ROOT_MANIFEST_PATHS,
      ...workspaceManifestPaths,
      ...(await findOptionalPnpmConfig(root)),
    ];
    for (const manifestPath of manifestPaths) {
      const target = resolveWithin(mirrorRoot, manifestPath);
      await mkdir(dirname(target), { recursive: true });
      await copyFile(resolveWithin(root, manifestPath), target);
    }
    return mirrorRoot;
  } catch (error: unknown) {
    await removeManifestMirror(mirrorRoot);
    throw error;
  }
}

async function removeManifestMirror(mirrorRoot: string): Promise<void> {
  const resolvedTempRoot = resolve(tmpdir());
  const resolvedMirrorRoot = resolve(mirrorRoot);
  if (
    dirname(resolvedMirrorRoot) !== resolvedTempRoot ||
    !basename(resolvedMirrorRoot).startsWith(MIRROR_PREFIX)
  ) {
    throw new Error(`Refusing to remove unsafe manifest mirror path: ${mirrorRoot}`);
  }
  await rm(resolvedMirrorRoot, { recursive: true, force: true });
}

export async function validateFrozenLockfile(
  root: string,
  runPnpm: PnpmRunner,
  workspaceManifestPaths: readonly string[],
): Promise<void> {
  const lockfile = await readFile(join(root, "pnpm-lock.yaml"), "utf8");
  if (lockfile.trim() === "") {
    throw new Error("pnpm-lock.yaml is empty");
  }
  const mirrorRoot = await createManifestMirror(root, workspaceManifestPaths);
  try {
    await runPnpm(mirrorRoot, FROZEN_LOCKFILE_ARGS);
  } finally {
    await removeManifestMirror(mirrorRoot);
  }
}
