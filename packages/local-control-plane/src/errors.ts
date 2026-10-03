export type ControlPlaneErrorCode =
  | "DESCRIPTOR_NOT_FOUND"
  | "DESCRIPTOR_TOO_LARGE"
  | "DUPLICATE_DESCRIPTOR_KEY"
  | "INSECURE_OWNER"
  | "INSECURE_PERMISSIONS"
  | "INVALID_CONFIG_FINGERPRINT"
  | "INVALID_DESCRIPTOR"
  | "INVALID_ENDPOINT"
  | "INVALID_TOKEN"
  | "INVALID_WORKSPACE_PATH"
  | "IO_FAILURE"
  | "LOCK_COMPROMISED"
  | "STALE_SERVER_EPOCH"
  | "SYMLINK_REJECTED"
  | "WINDOWS_ACL_REQUIRED"
  | "WORKSPACE_NOT_ABSOLUTE"
  | "WORKSPACE_NOT_DIRECTORY"
  | "WORKSPACE_NOT_FOUND"
  | "WRONG_FILE_TYPE";

const SAFE_MESSAGES: Readonly<Record<ControlPlaneErrorCode, string>> = Object.freeze({
  DESCRIPTOR_NOT_FOUND: "local daemon discovery descriptor was not found",
  DESCRIPTOR_TOO_LARGE: "local daemon discovery descriptor exceeds the size limit",
  DUPLICATE_DESCRIPTOR_KEY: "local daemon discovery descriptor contains a duplicate field",
  INSECURE_OWNER: "local control-plane path is not owned by the current user",
  INSECURE_PERMISSIONS: "local control-plane path has insecure permissions",
  INVALID_CONFIG_FINGERPRINT: "effective configuration fingerprint is invalid",
  INVALID_DESCRIPTOR: "local daemon discovery descriptor is invalid",
  INVALID_ENDPOINT: "local daemon endpoint must be a numeric loopback address with a non-zero port",
  INVALID_TOKEN: "local authentication token is invalid",
  INVALID_WORKSPACE_PATH: "workspace path is invalid",
  IO_FAILURE: "local control-plane I/O failed",
  LOCK_COMPROMISED: "local daemon ownership lease was compromised",
  STALE_SERVER_EPOCH: "command is bound to a stale server epoch",
  SYMLINK_REJECTED: "symbolic links and reparse points are not allowed in the local control plane",
  WINDOWS_ACL_REQUIRED: "a verified Windows owner-only DACL adapter is required",
  WORKSPACE_NOT_ABSOLUTE: "workspace path must be absolute",
  WORKSPACE_NOT_DIRECTORY: "workspace path must identify a directory",
  WORKSPACE_NOT_FOUND: "workspace path does not exist",
  WRONG_FILE_TYPE: "local control-plane path has the wrong file type",
});

export class ControlPlaneError extends Error {
  public override readonly name = "ControlPlaneError";

  public constructor(
    public readonly code: ControlPlaneErrorCode,
    options?: ErrorOptions,
  ) {
    super(SAFE_MESSAGES[code], options);
  }
}

export function controlPlaneError(code: ControlPlaneErrorCode, cause?: unknown): ControlPlaneError {
  return cause === undefined ? new ControlPlaneError(code) : new ControlPlaneError(code, { cause });
}

export function isNodeErrorWithCode(error: unknown, code: string): boolean {
  return error instanceof Error && "code" in error && error.code === code;
}
