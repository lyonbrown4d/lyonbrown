import { createHash } from "node:crypto";
import { isAbsolute, win32 } from "node:path";
import { controlPlaneError } from "./errors.js";
import {
  invokePlatform,
  type PlatformControlPlaneSecurity,
  type WorkspaceIdentity,
} from "./platform-security.js";
import { isWellFormedText } from "./well-formed.js";

export interface ScopeIdentity {
  readonly canonicalWorkspace: string;
  readonly scopeId: string;
}

function framedLength(length: number): Buffer {
  const bytes = Buffer.alloc(8);
  bytes.writeBigUInt64LE(BigInt(length));
  return bytes;
}

function validateText(
  value: string,
  code: "INVALID_WORKSPACE_PATH" | "INVALID_CONFIG_FINGERPRINT",
) {
  if (value.includes("\0") || !isWellFormedText(value)) throw controlPlaneError(code);
}

function unsignedBigEndian(value: bigint, bytes: 4 | 8 | 16): Buffer {
  const maximum = 1n << BigInt(bytes * 8);
  if (value < 0n || value >= maximum) throw controlPlaneError("INVALID_WORKSPACE_PATH");
  const output = Buffer.alloc(bytes);
  let remaining = value;
  for (let index = bytes - 1; index >= 0; index -= 1) {
    output[index] = Number(remaining & 0xffn);
    remaining >>= 8n;
  }
  return output;
}

export function encodeWorkspaceIdentity(identity: WorkspaceIdentity): Uint8Array {
  const prefix = Buffer.from("workspace-file-id-v1\0", "utf8");
  switch (identity.kind) {
    case "inode":
      return Buffer.concat([
        prefix,
        Buffer.from([1]),
        unsignedBigEndian(identity.deviceId, 8),
        unsignedBigEndian(identity.inodeNumber, 8),
      ]);
    case "windows-low-res":
      return Buffer.concat([
        prefix,
        Buffer.from([2]),
        unsignedBigEndian(identity.volumeSerialNumber, 4),
        unsignedBigEndian(identity.fileIndex, 8),
      ]);
    case "windows-high-res":
      return Buffer.concat([
        prefix,
        Buffer.from([3]),
        unsignedBigEndian(identity.volumeSerialNumber, 8),
        unsignedBigEndian(identity.fileId, 16),
      ]);
  }
}

export function deriveScopeFromIdentity(
  workspaceIdentity: Uint8Array,
  effectiveConfigFingerprint: string,
): string {
  validateText(effectiveConfigFingerprint, "INVALID_CONFIG_FINGERPRINT");
  if (effectiveConfigFingerprint.trim().length === 0) {
    throw controlPlaneError("INVALID_CONFIG_FINGERPRINT");
  }
  const identityBytes = Buffer.from(workspaceIdentity);
  const configBytes = Buffer.from(effectiveConfigFingerprint, "utf8");
  return createHash("sha256")
    .update("lyonbrown-agentd-scope-v1\0")
    .update(framedLength(identityBytes.length))
    .update(identityBytes)
    .update(framedLength(configBytes.length))
    .update(configBytes)
    .digest("hex");
}

function absoluteForPlatform(path: string, platform: NodeJS.Platform): boolean {
  return platform === "win32" ? win32.isAbsolute(path) : isAbsolute(path);
}

export async function deriveScope(
  workspace: string,
  effectiveConfigFingerprint: string,
  security?: PlatformControlPlaneSecurity,
  platform: NodeJS.Platform = process.platform,
): Promise<ScopeIdentity> {
  validateText(workspace, "INVALID_WORKSPACE_PATH");
  if (!absoluteForPlatform(workspace, platform)) throw controlPlaneError("WORKSPACE_NOT_ABSOLUTE");
  const parsed = platform === "win32" ? win32.parse(workspace) : undefined;
  const segments = (platform === "win32" ? workspace.slice(parsed?.root.length) : workspace).split(
    /[\\/]/u,
  );
  if (segments.some((segment) => segment === "." || segment === "..")) {
    throw controlPlaneError("INVALID_WORKSPACE_PATH");
  }
  validateText(effectiveConfigFingerprint, "INVALID_CONFIG_FINGERPRINT");
  if (effectiveConfigFingerprint.trim().length === 0) {
    throw controlPlaneError("INVALID_CONFIG_FINGERPRINT");
  }

  if (!security) throw controlPlaneError("NATIVE_SECURITY_PROVIDER_REQUIRED");
  const identity = await invokePlatform("workspace_identity", () =>
    security.workspaceIdentity(workspace),
  );
  const scopeId = deriveScopeFromIdentity(
    encodeWorkspaceIdentity(identity),
    effectiveConfigFingerprint,
  );
  return Object.freeze({ canonicalWorkspace: workspace, scopeId });
}
