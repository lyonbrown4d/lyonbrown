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
 * Implementations must bind every anchor to the complete identity, reject
 * reparse points and identity drift, and deny write/delete while callbacks run.
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
