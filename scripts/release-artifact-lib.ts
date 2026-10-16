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
  if (
    !["win32", "darwin", "linux"].includes(platform) ||
    !["x64", "arm64"].includes(architecture)
  ) {
    throw new Error(`Unsupported release target: ${platform}-${architecture}`);
  }

  const args = arguments_[0] === "--" ? arguments_.slice(1) : arguments_;
  let requested = `artifacts/lyonbrown-${platform}-${architecture}`;
  if (args.length > 0) {
    if (args.length !== 2 || args[0] !== "--output" || args[1] === undefined) {
      throw new Error("Usage: pnpm package:release -- --output artifacts/<name>");
    }
    requested = args[1];
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

export function resolveArtifactSmokeDirectory(arguments_: readonly string[]): string {
  const args = arguments_[0] === "--" ? arguments_.slice(1) : arguments_;
  if (args.length !== 2 || args[0] !== "--artifact" || args[1] === undefined) {
    throw new Error("Usage: smoke-release-artifact --artifact <directory>");
  }
  return resolve(args[1]);
}

export function windowsAgentLauncher(): string {
  return ["@echo off", "setlocal", '"%~dp0runtime\\node.exe" "%~dp0dist\\main.js" %*', ""].join(
    "\r\n",
  );
}

export function releaseRuntimeName(platform: NodeJS.Platform): string {
  assertSupportedPlatform(platform);
  return platform === "win32" ? "node.exe" : "node";
}

export function releaseLauncherName(platform: NodeJS.Platform): string {
  assertSupportedPlatform(platform);
  return platform === "win32" ? "agent.cmd" : "agent";
}

function assertSupportedPlatform(platform: NodeJS.Platform): void {
  if (!["win32", "darwin", "linux"].includes(platform)) {
    throw new Error(`Unsupported release platform: ${platform}`);
  }
}

export function posixAgentLauncher(): string {
  return [
    "#!/bin/sh",
    'agent_dir=$(CDPATH= cd -P "$(dirname "$0")" && pwd) || exit 1',
    'exec "$agent_dir/runtime/node" "$agent_dir/dist/main.js" "$@"',
    "",
  ].join("\n");
}
