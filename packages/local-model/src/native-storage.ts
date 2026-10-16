export interface NativeModelIdentity {
  readonly modelId: string;
  readonly revision: string;
  readonly sha256: string;
  readonly filename: string;
  readonly maxBytes: number;
}

export interface NativeInstallLockAnchor {
  /** Keeps the native lock target anchored until `release`, beyond this callback. */
  withLockTarget<T>(use: (lockTargetPath: string) => Promise<T>): Promise<T>;
  release(): Promise<void>;
}

export interface NativeModelStagingFile {
  /** The file and directory are same-filesystem and stable until cleanup. */
  withFilePath<T>(use: (filePath: string, directoryPath: string) => Promise<T>): Promise<T>;
  /** Atomically replaces the bound installed identity, including on Windows. */
  activate(): Promise<void>;
  cleanup(): Promise<void>;
}

export type NativeInstalledFileResult<T> =
  | { readonly kind: "missing" }
  | { readonly kind: "present"; readonly value: T };

/**
 * Privileged filesystem operations supplied by the native control plane.
 * Implementations bind anchors to the complete identity, reject links and identity
 * drift, and exclude conflicting adapter-managed activation while read callbacks
 * run. Windows also enforces native share-deny. POSIX uses private owner-only
 * directories, kernel advisory locks, and pre/post identity/content checks; it
 * does not isolate a hostile process running as the same OS user.
 */
export interface NativeModelStorageCapability {
  acquireInstallLockAnchor(
    identity: NativeModelIdentity,
    signal: AbortSignal,
  ): Promise<NativeInstallLockAnchor>;
  createStagingFile(
    identity: NativeModelIdentity,
    operationId: string,
    signal: AbortSignal,
  ): Promise<NativeModelStagingFile>;
  withInstalledFile<T>(
    identity: NativeModelIdentity,
    signal: AbortSignal,
    use: (filePath: string) => Promise<T>,
  ): Promise<NativeInstalledFileResult<T>>;
}
