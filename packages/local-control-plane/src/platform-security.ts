import { ControlPlaneError, controlPlaneError, type ControlPlaneOperation } from "./errors.ts";

export type PlatformFailureCode =
  | "access_denied"
  | "insecure_acl"
  | "insecure_mode"
  | "io"
  | "not_directory"
  | "not_found"
  | "oversize"
  | "reparse_point"
  | "wrong_owner"
  | "wrong_type";

export type PlatformResult<T> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly code: PlatformFailureCode };

export type WorkspaceIdentity =
  | { readonly kind: "inode"; readonly deviceId: bigint; readonly inodeNumber: bigint }
  | {
      readonly kind: "windows-low-res";
      readonly volumeSerialNumber: bigint;
      readonly fileIndex: bigint;
    }
  | {
      readonly kind: "windows-high-res";
      readonly volumeSerialNumber: bigint;
      readonly fileId: bigint;
    };

export interface ResolvedWorkspaceIdentity {
  /** Absolute, well-formed native canonical path for the opened directory. */
  readonly canonicalPath: string;
  readonly identity: WorkspaceIdentity;
}

export interface KernelFileLock {
  release(): Promise<PlatformResult<void>>;
}

export interface SecureDirectoryHandle {
  /** Reads an opened, owner-verified regular file without following links. */
  readPrivateFile(
    name: "agentd.json",
    maximumBytes: number,
  ): Promise<PlatformResult<Uint8Array | null>>;
  /** Uses a random same-directory temporary, fsync, atomic replace, and cleanup. */
  atomicReplacePrivateFile(name: "agentd.json", bytes: Uint8Array): Promise<PlatformResult<void>>;
  /** Acquires an owner-bound OS kernel lock on an ordinary relative file. */
  tryAcquireKernelLock(
    name: "agentd.start.lock" | "agentd.lock",
  ): Promise<PlatformResult<KernelFileLock | null>>;
  close(): Promise<PlatformResult<void>>;
}

export type PlatformSecurityContract =
  | {
      readonly platform: "windows";
      readonly directoryOpen: "FILE_FLAG_OPEN_REPARSE_POINT";
      readonly ownerProtection: "current-user-only-dacl";
      readonly workspaceIdentity: "native-file-id-v1";
      readonly kernelLock: "kernel-file-lock-v1";
    }
  | {
      readonly platform: "posix";
      readonly directoryOpen: "dirfd-openat-O_NOFOLLOW";
      readonly ownerProtection: "current-uid-owner-mode";
      readonly workspaceIdentity: "native-dev-ino-v1";
      readonly kernelLock: "kernel-file-lock-v1";
    };

/** Native boundary: implementations must anchor all operations to verified handles. */
export interface PlatformControlPlaneSecurity {
  readonly contract: PlatformSecurityContract;
  workspaceIdentity(path: string): Promise<PlatformResult<ResolvedWorkspaceIdentity>>;
  openSecureRunDirectory(path: string): Promise<PlatformResult<SecureDirectoryHandle>>;
}

declare const nativeProviderBrand: unique symbol;

export interface NativeControlPlaneProvider {
  readonly [nativeProviderBrand]: "NativeControlPlaneProvider";
}

const NATIVE_PROVIDERS = new WeakMap<NativeControlPlaneProvider, PlatformControlPlaneSecurity>();

export function createNativeControlPlaneProvider(
  adapter: PlatformControlPlaneSecurity,
): NativeControlPlaneProvider {
  const provider = Object.freeze({}) as NativeControlPlaneProvider;
  NATIVE_PROVIDERS.set(provider, adapter);
  return provider;
}

export function resolveNativeControlPlaneProvider(
  provider: NativeControlPlaneProvider,
): PlatformControlPlaneSecurity {
  const adapter = NATIVE_PROVIDERS.get(provider);
  if (!adapter) throw controlPlaneError("NATIVE_SECURITY_PROVIDER_REQUIRED");
  return adapter;
}

export function unwrapPlatformResult<T>(
  result: PlatformResult<T>,
  operation: ControlPlaneOperation,
): T {
  if (!result.ok) throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation });
  return result.value;
}

export async function invokePlatform<T>(
  operation: ControlPlaneOperation,
  invoke: () => Promise<PlatformResult<T>>,
): Promise<T> {
  try {
    return unwrapPlatformResult(await invoke(), operation);
  } catch (error) {
    if (error instanceof ControlPlaneError) throw error;
    throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation });
  }
}
