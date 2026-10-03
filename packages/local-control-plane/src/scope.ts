import { createHash } from "node:crypto";
import { realpath, stat } from "node:fs/promises";
import { isAbsolute } from "node:path";
import { controlPlaneError, isNodeErrorWithCode } from "./errors.js";
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

export function normalizeCanonicalWorkspacePath(
  canonicalWorkspace: string,
  platform: NodeJS.Platform,
): string {
  return platform === "win32"
    ? canonicalWorkspace.replaceAll("\\", "/").toLocaleLowerCase("en-US")
    : canonicalWorkspace;
}

function validateText(
  value: string,
  code: "INVALID_WORKSPACE_PATH" | "INVALID_CONFIG_FINGERPRINT",
) {
  if (value.includes("\0") || !isWellFormedText(value)) throw controlPlaneError(code);
}

export async function deriveScope(
  workspace: string,
  effectiveConfigFingerprint: string,
  platform: NodeJS.Platform = process.platform,
): Promise<ScopeIdentity> {
  validateText(workspace, "INVALID_WORKSPACE_PATH");
  if (!isAbsolute(workspace)) throw controlPlaneError("WORKSPACE_NOT_ABSOLUTE");
  validateText(effectiveConfigFingerprint, "INVALID_CONFIG_FINGERPRINT");
  if (effectiveConfigFingerprint.trim().length === 0) {
    throw controlPlaneError("INVALID_CONFIG_FINGERPRINT");
  }

  let canonicalWorkspace: string;
  try {
    canonicalWorkspace = await realpath(workspace);
  } catch (error) {
    if (isNodeErrorWithCode(error, "ENOENT")) throw controlPlaneError("WORKSPACE_NOT_FOUND");
    throw controlPlaneError("IO_FAILURE", error);
  }
  const metadata = await stat(canonicalWorkspace).catch((error: unknown) => {
    throw controlPlaneError("IO_FAILURE", error);
  });
  if (!metadata.isDirectory()) throw controlPlaneError("WORKSPACE_NOT_DIRECTORY");

  const normalized = normalizeCanonicalWorkspacePath(canonicalWorkspace, platform);
  const workspaceBytes = Buffer.from(normalized, "utf8");
  const configBytes = Buffer.from(effectiveConfigFingerprint, "utf8");
  const scopeId = createHash("sha256")
    .update("lyonbrown-agentd-scope-v1\0")
    .update(framedLength(workspaceBytes.length))
    .update(workspaceBytes)
    .update(framedLength(configBytes.length))
    .update(configBytes)
    .digest("hex");

  return Object.freeze({ canonicalWorkspace, scopeId });
}
