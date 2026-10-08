export interface DecisionModelBundleFileIdentity {
  readonly path: string;
  readonly sha256: string;
  readonly maxBytes: number;
}

export interface DecisionModelBundleIdentity {
  readonly modelId: string;
  readonly revision: string;
  readonly files: readonly DecisionModelBundleFileIdentity[];
}

export interface DecisionModelBundleInstallLock {
  withLockTarget<T>(use: (lockTargetPath: string) => Promise<T>): Promise<T>;
  release(): Promise<void>;
}

export interface DecisionModelStagingBundle {
  withDirectoryPath<T>(use: (directoryPath: string) => Promise<T>): Promise<T>;
  activate(): Promise<void>;
  cleanup(): Promise<void>;
}

export type DecisionInstalledBundleResult<T> =
  | { readonly kind: "missing" }
  | { readonly kind: "present"; readonly value: T };

export interface DecisionModelBundleStoragePort {
  acquireInstallLockAnchor(
    identity: DecisionModelBundleIdentity,
    signal: AbortSignal,
  ): Promise<DecisionModelBundleInstallLock>;
  createStagingBundle(
    identity: DecisionModelBundleIdentity,
    operationId: string,
    signal: AbortSignal,
  ): Promise<DecisionModelStagingBundle>;
  withInstalledBundle<T>(
    identity: DecisionModelBundleIdentity,
    signal: AbortSignal,
    use: (directoryPath: string) => Promise<T>,
  ): Promise<DecisionInstalledBundleResult<T>>;
}
