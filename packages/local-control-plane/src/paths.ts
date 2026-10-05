import { isAbsolute, join, win32 } from "node:path";

import { controlPlaneError } from "./errors.ts";
import {
  invokePlatform,
  resolveNativeControlPlaneProvider,
  type NativeControlPlaneProvider,
  type ResolvedWorkspaceIdentity,
  type WorkspaceIdentity,
} from "./platform-security.ts";
import { deriveScopeFromIdentity, encodeWorkspaceIdentity } from "./scope.ts";
import { isWellFormedText } from "./well-formed.ts";

export interface ScopedDaemonPaths {
  readonly canonicalWorkspace: string;
  readonly workspaceCacheDirectory: string;
  readonly dataDirectory: string;
  readonly runDirectory: string;
  readonly scopeId: string;
}

function absoluteForPlatform(path: string, platform: NodeJS.Platform): boolean {
  return platform === "win32" ? win32.isAbsolute(path) : isAbsolute(path);
}

function validateAbsolutePath(path: string, platform: NodeJS.Platform): void {
  if (path.includes("\0") || !isWellFormedText(path) || !absoluteForPlatform(path, platform)) {
    throw controlPlaneError("INVALID_WORKSPACE_PATH");
  }
  const parsed = platform === "win32" ? win32.parse(path) : undefined;
  const tail = platform === "win32" ? path.slice(parsed?.root.length) : path;
  if (tail.split(/[\\/]/u).some((segment) => segment === "." || segment === "..")) {
    throw controlPlaneError("INVALID_WORKSPACE_PATH");
  }
}

function snapshotIdentity(identity: WorkspaceIdentity): WorkspaceIdentity {
  switch (identity.kind) {
    case "inode":
      return {
        kind: identity.kind,
        deviceId: identity.deviceId,
        inodeNumber: identity.inodeNumber,
      };
    case "windows-low-res":
      return {
        kind: identity.kind,
        volumeSerialNumber: identity.volumeSerialNumber,
        fileIndex: identity.fileIndex,
      };
    case "windows-high-res":
      return {
        kind: identity.kind,
        volumeSerialNumber: identity.volumeSerialNumber,
        fileId: identity.fileId,
      };
  }
}

function stableResolvedIdentity(
  value: ResolvedWorkspaceIdentity,
  platform: NodeJS.Platform,
): { readonly canonicalPath: string; readonly encoded: Uint8Array } {
  try {
    const firstPath = value.canonicalPath;
    const first = encodeWorkspaceIdentity(snapshotIdentity(value.identity));
    const secondPath = value.canonicalPath;
    const second = encodeWorkspaceIdentity(snapshotIdentity(value.identity));
    validateAbsolutePath(firstPath, platform);
    if (firstPath !== secondPath || !Buffer.from(first).equals(Buffer.from(second))) {
      throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation: "workspace_identity" });
    }
    return { canonicalPath: firstPath, encoded: first };
  } catch {
    throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation: "workspace_identity" });
  }
}

function fnv1a64(bytes: Uint8Array): string {
  let hash = 0xcbf2_9ce4_8422_2325n;
  for (const byte of bytes) {
    hash ^= BigInt(byte);
    hash = BigInt.asUintN(64, hash * 0x0100_0000_01b3n);
  }
  return hash.toString(16).padStart(16, "0");
}

export async function deriveScopedDaemonPaths(
  homeDirectory: string,
  workspace: string,
  effectiveConfigFingerprint: string,
  provider?: NativeControlPlaneProvider,
  platform: NodeJS.Platform = process.platform,
): Promise<ScopedDaemonPaths> {
  validateAbsolutePath(homeDirectory, platform);
  validateAbsolutePath(workspace, platform);
  if (
    effectiveConfigFingerprint.includes("\0") ||
    !isWellFormedText(effectiveConfigFingerprint) ||
    effectiveConfigFingerprint.trim().length === 0
  ) {
    throw controlPlaneError("INVALID_CONFIG_FINGERPRINT");
  }
  if (!provider) throw controlPlaneError("NATIVE_SECURITY_PROVIDER_REQUIRED");
  const security = resolveNativeControlPlaneProvider(provider);
  const resolved = await invokePlatform("workspace_identity", () =>
    security.workspaceIdentity(workspace),
  );
  const stable = stableResolvedIdentity(resolved, platform);
  const scopeId = deriveScopeFromIdentity(stable.encoded, effectiveConfigFingerprint);
  const pathJoin = (...segments: readonly string[]): string =>
    platform === "win32" ? win32.join(...segments) : join(...segments);
  const workspaceCacheDirectory = pathJoin(
    homeDirectory,
    ".agent",
    "workspaces",
    fnv1a64(stable.encoded),
  );
  return Object.freeze({
    canonicalWorkspace: stable.canonicalPath,
    workspaceCacheDirectory,
    dataDirectory: pathJoin(workspaceCacheDirectory, "data"),
    runDirectory: pathJoin(workspaceCacheDirectory, "run", scopeId),
    scopeId,
  });
}
