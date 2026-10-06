export interface LocalModelDescriptor {
  readonly id: string;
  readonly displayName: string;
  readonly expectedDownloadBytes: number;
  readonly maxBytes: number;
  readonly minimumMemoryBytes: number;
  readonly sha256: string;
}

export type LocalModelSelectionReason =
  | "installed-preferred"
  | "hardware-compatible"
  | "conservative-fallback";

export const LOCAL_MODEL_MAX_MESSAGE_BYTES = 64 * 1024;

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
  readonly selectionReason: LocalModelSelectionReason | null;
  readonly allowedActions: readonly LocalModelAction[];
}

export type LocalModelMessage =
  | { readonly role: "system"; readonly content: string }
  | { readonly role: "user"; readonly content: string }
  | { readonly role: "assistant"; readonly content: string };

export interface LocalModelInferenceRequest {
  readonly messages: readonly LocalModelMessage[];
  readonly signal: AbortSignal;
  readonly maxOutputTokens: number;
  readonly maxOutputBytes: number;
}

export type LocalModelInferenceErrorCode =
  | "disposed"
  | "invalid_request"
  | "native_inference_failed"
  | "resource_busy"
  | "unsupported_tool_use";

export type LocalModelInferenceEvent =
  | { readonly type: "text_delta"; readonly text: string }
  | { readonly type: "usage"; readonly inputTokens: number; readonly outputTokens: number }
  | { readonly type: "done"; readonly reason: "stop" | "length" | "cancelled" }
  | {
      readonly type: "error";
      readonly code: LocalModelInferenceErrorCode;
      readonly message: string;
    };

export interface LocalModelLease {
  stream(request: LocalModelInferenceRequest): AsyncIterable<LocalModelInferenceEvent>;
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
