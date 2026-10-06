import { relative, resolve } from "node:path";

export interface ReleaseTarget {
  readonly outputDirectory: string;
  readonly platform: NodeJS.Platform;
  readonly architecture: NodeJS.Architecture;
}

export function resolveReleaseTarget(
  workspaceRoot: string,
  arguments_: readonly string[],
  platform: NodeJS.Platform = process.platform,
  architecture: NodeJS.Architecture = process.arch,
): ReleaseTarget {
  if (platform !== "win32" || !["x64", "arm64"].includes(architecture)) {
    throw new Error(`Unsupported release target: ${platform}-${architecture}`);
  }

  let requested = `artifacts/lyonbrown-${platform}-${architecture}`;
  if (arguments_.length > 0) {
    if (arguments_.length !== 2 || arguments_[0] !== "--output" || arguments_[1] === undefined) {
      throw new Error("Usage: pnpm package:release -- --output artifacts/<name>");
    }
    requested = arguments_[1];
  }

  const artifactsRoot = resolve(workspaceRoot, "artifacts");
  const outputDirectory = resolve(workspaceRoot, requested);
  const nestedPath = relative(artifactsRoot, outputDirectory);
  if (
    nestedPath === "" ||
    nestedPath.startsWith("..") ||
    resolve(artifactsRoot, nestedPath) !== outputDirectory
  ) {
    throw new Error("Release output must be a child of the workspace artifacts directory");
  }

  return { outputDirectory, platform, architecture };
}

export function windowsAgentLauncher(): string {
  return ["@echo off", "setlocal", '"%~dp0runtime\\node.exe" "%~dp0dist\\main.js" %*', ""].join(
    "\r\n",
  );
}
