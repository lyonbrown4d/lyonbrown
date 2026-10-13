export interface DecisionModelDto {
  readonly providerId: string;
  readonly modelId: string;
  readonly modelRevision: string;
  readonly advisoryOnly: true;
  readonly phase:
    | "unavailable"
    | "checking"
    | "required"
    | "waiting-for-lock"
    | "downloading"
    | "verifying"
    | "installing"
    | "configured"
    | "cancelled"
    | "failed";
  readonly operationId: string | null;
  readonly downloadedBytes: bigint;
  readonly totalBytes: bigint;
  readonly failure: {
    readonly code: string;
    readonly message: string;
    readonly retryable: boolean;
  } | null;
  readonly allowedActions: readonly ("download" | "cancel" | "retry")[];
}
