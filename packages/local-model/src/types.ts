export interface LocalModelDescriptor {
  readonly id: "qwen3-1.7b-q4_k_m";
  readonly displayName: string;
  readonly expectedDownloadBytes: number;
  readonly minimumMemoryBytes: number;
}

export type LocalModelPhase =
  | "checking"
  | "setup-required"
  | "waiting-for-lock"
  | "downloading"
  | "verifying"
  | "installing"
  | "ready"
  | "cancelled"
  | "failed";

export type LocalModelAction = "download" | "cancel" | "retry";

export interface LocalModelStatus {
  readonly model: LocalModelDescriptor;
  readonly phase: LocalModelPhase;
  readonly operationId: string | null;
  readonly downloadedBytes: number;
  readonly totalBytes: number | null;
  readonly failure: null | {
    readonly code: "cache_inspection_failed" | "download_failed";
    readonly retryable: true;
  };
  readonly allowedActions: readonly LocalModelAction[];
}

export interface LocalModelLease {
  dispose(): Promise<void>;
}

export type LocalModelLoadResult =
  | { readonly kind: "checking" }
  | { readonly kind: "setup-required" }
  | {
      readonly kind: "unavailable";
      readonly code: "native_runtime_unavailable" | "service_disposed";
    }
  | {
      readonly kind: "ready";
      readonly model: LocalModelDescriptor;
      readonly lease: LocalModelLease;
    };

export type StartModelDownloadResult =
  | { readonly kind: "started"; readonly operationId: string }
  | { readonly kind: "in-progress"; readonly operationId: string }
  | { readonly kind: "already-ready" }
  | { readonly kind: "checking" }
  | { readonly kind: "disposed" };

export type CancelModelDownloadResult =
  | { readonly kind: "cancelled" }
  | { readonly kind: "no-active-operation" }
  | { readonly kind: "operation-mismatch"; readonly activeOperationId: string }
  | { readonly kind: "too-late" };

export interface LocalModelService {
  status(): LocalModelStatus;
  subscribe(listener: (status: LocalModelStatus) => void): () => void;
  startDownload(): StartModelDownloadResult;
  cancelDownload(operationId: string): CancelModelDownloadResult;
  load(): Promise<LocalModelLoadResult>;
  dispose(): Promise<void>;
}
