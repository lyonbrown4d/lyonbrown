export interface ModelPreset {
  readonly id: "qwen3-1.7b-q4_k_m";
  readonly displayName: string;
  readonly owner: "Qwen";
  readonly repository: "Qwen3-1.7B-GGUF";
  readonly revision: string;
  readonly filename: "Qwen3-1.7B-Q4_K_M.gguf";
  readonly sha256: string;
  readonly expectedDownloadBytes: number;
  readonly maxBytes: number;
  readonly minimumMemoryBytes: number;
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

export interface LlamaLease {
  dispose(): Promise<void>;
}

export interface LlamaEngineAdapter {
  loadDefault(): Promise<LlamaLease>;
}
