import type { LocalModelLease } from "@lyonbrown/local-model/internal/types";

export interface ModelPreset {
  readonly id: string;
  readonly displayName: string;
  readonly owner: string;
  readonly repository: string;
  readonly revision: string;
  readonly filename: string;
  readonly sha256: string;
  readonly expectedDownloadBytes: number;
  readonly maxBytes: number;
  readonly minimumMemoryBytes: number;
  readonly minimumContextTokens: number;
  readonly qualityRank: number;
  readonly license: "apache-2.0";
}

export interface ModelCompatibilityEvaluation {
  readonly modelId: string;
  readonly compatibilityScore: number;
  readonly bonusScore: number;
}

export interface ModelCompatibilityEvaluator {
  evaluate(
    presets: readonly ModelPreset[],
    signal: AbortSignal,
  ): Promise<readonly ModelCompatibilityEvaluation[]>;
}

export interface ModelSelection {
  readonly preset: ModelPreset;
  readonly source: "hardware" | "fallback";
}

export interface ModelSelector {
  select(presets: readonly ModelPreset[], signal: AbortSignal): Promise<ModelSelection>;
}

export interface DownloadProgress {
  readonly downloadedBytes: number;
  readonly totalBytes: number | null;
}

export interface DownloadArtifact {
  readonly operationId: string;
  readonly token: symbol;
}

export interface LocalModelIo {
  inspectInstalled(preset: ModelPreset, signal: AbortSignal): Promise<boolean>;
  acquireInstallLock(preset: ModelPreset, signal: AbortSignal): Promise<InstallLockLease>;
  download(
    preset: ModelPreset,
    request: {
      readonly operationId: string;
      readonly signal: AbortSignal;
      readonly onProgress: (progress: DownloadProgress) => void;
    },
  ): Promise<DownloadArtifact>;
  verify(artifact: DownloadArtifact, preset: ModelPreset, signal: AbortSignal): Promise<number>;
  activate(artifact: DownloadArtifact, preset: ModelPreset): Promise<void>;
  cleanup(artifact: DownloadArtifact | undefined): Promise<void>;
}

export interface InstallLockLease {
  release(): Promise<void>;
}

export type LlamaLease = LocalModelLease;

export interface LlamaEngineAdapter {
  loadDefault(signal: AbortSignal): Promise<LlamaLease>;
  load?(preset: ModelPreset, signal: AbortSignal): Promise<LlamaLease>;
}
