export interface NativeModelBundleFileIdentity {
  /** Slash-separated path relative to the immutable bundle root. */
  readonly path: string;
  readonly sha256: string;
  readonly maxBytes: number;
}

export interface NativeModelBundleIdentity {
  readonly modelId: string;
  readonly revision: string;
  readonly files: readonly NativeModelBundleFileIdentity[];
}

export interface NativeModelBundleInstallLock {
  withLockTarget<T>(use: (lockTargetPath: string) => Promise<T>): Promise<T>;
  release(): Promise<void>;
}

export interface NativeModelStagingBundle {
  /** The directory is same-filesystem and remains anchored until cleanup. */
  withDirectoryPath<T>(use: (directoryPath: string) => Promise<T>): Promise<T>;
  /** Atomically publishes the complete immutable directory. */
  activate(): Promise<void>;
  cleanup(): Promise<void>;
}

export type NativeInstalledBundleResult<T> =
  | { readonly kind: "missing" }
  | { readonly kind: "present"; readonly value: T };

export interface NativeModelBundleStorageCapability {
  acquireInstallLockAnchor(
    identity: NativeModelBundleIdentity,
    signal: AbortSignal,
  ): Promise<NativeModelBundleInstallLock>;
  createStagingBundle(
    identity: NativeModelBundleIdentity,
    operationId: string,
    signal: AbortSignal,
  ): Promise<NativeModelStagingBundle>;
  withInstalledBundle<T>(
    identity: NativeModelBundleIdentity,
    signal: AbortSignal,
    use: (directoryPath: string) => Promise<T>,
  ): Promise<NativeInstalledBundleResult<T>>;
}
