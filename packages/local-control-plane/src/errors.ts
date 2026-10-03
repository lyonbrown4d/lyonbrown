export type ControlPlaneErrorCode =
  | "DESCRIPTOR_NOT_FOUND"
  | "DESCRIPTOR_TOO_LARGE"
  | "DUPLICATE_DESCRIPTOR_KEY"
  | "INVALID_CONFIG_FINGERPRINT"
  | "INVALID_DESCRIPTOR"
  | "INVALID_ENDPOINT"
  | "INVALID_TOKEN"
  | "INVALID_WORKSPACE_PATH"
  | "NATIVE_SECURITY_PROVIDER_REQUIRED"
  | "PLATFORM_SECURITY_FAILURE"
  | "STALE_SERVER_EPOCH"
  | "WORKSPACE_NOT_ABSOLUTE";

export type ControlPlaneOperation =
  | "atomic_replace"
  | "close_directory"
  | "kernel_lock"
  | "open_run_directory"
  | "read_descriptor"
  | "release_lock"
  | "workspace_identity";

export interface ControlPlaneErrorDetails {
  readonly operation: ControlPlaneOperation;
}

const SAFE_MESSAGES: Readonly<Record<ControlPlaneErrorCode, string>> = Object.freeze({
  DESCRIPTOR_NOT_FOUND: "local daemon discovery descriptor was not found",
  DESCRIPTOR_TOO_LARGE: "local daemon discovery descriptor exceeds the size limit",
  DUPLICATE_DESCRIPTOR_KEY: "local daemon discovery descriptor contains a duplicate field",
  INVALID_CONFIG_FINGERPRINT: "effective configuration fingerprint is invalid",
  INVALID_DESCRIPTOR: "local daemon discovery descriptor is invalid",
  INVALID_ENDPOINT: "local daemon endpoint must be a numeric loopback address with a non-zero port",
  INVALID_TOKEN: "local authentication token is invalid",
  INVALID_WORKSPACE_PATH: "workspace path is invalid",
  NATIVE_SECURITY_PROVIDER_REQUIRED: "a native secure control-plane provider is required",
  PLATFORM_SECURITY_FAILURE: "the native secure control-plane operation failed",
  STALE_SERVER_EPOCH: "command is bound to a stale server epoch",
  WORKSPACE_NOT_ABSOLUTE: "workspace path must be absolute",
});

export class ControlPlaneError extends Error {
  public override readonly name = "ControlPlaneError";

  public constructor(
    public readonly code: ControlPlaneErrorCode,
    public readonly details?: ControlPlaneErrorDetails,
  ) {
    super(SAFE_MESSAGES[code]);
    Object.freeze(this);
  }
}

export function controlPlaneError(
  code: ControlPlaneErrorCode,
  details?: ControlPlaneErrorDetails,
): ControlPlaneError {
  return new ControlPlaneError(code, details === undefined ? undefined : Object.freeze(details));
}
