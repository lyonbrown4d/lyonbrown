import type { EpochBinding } from "@lyonbrown/local-control-plane";

export type ApprovalDecision = "allow-once" | "allow-for-session" | "deny-once" | "deny-remaining";

export type ModelSelectionDto =
  | { readonly kind: "auto" }
  | { readonly kind: "pinned"; readonly provider: string; readonly model: string };

export type AgentCommand =
  | { readonly type: "start-turn"; readonly text: string }
  | { readonly type: "steer-current-turn"; readonly text: string }
  | { readonly type: "queue-follow-up"; readonly text: string }
  | { readonly type: "cancel-turn" }
  | { readonly type: "force-stop" }
  | {
      readonly type: "respond-approval";
      readonly requestId: string;
      readonly decision: ApprovalDecision;
    }
  | { readonly type: "select-session-model"; readonly selection: ModelSelectionDto }
  | { readonly type: "start-model-download"; readonly modelId: string }
  | { readonly type: "cancel-model-download"; readonly operationId: string };

export interface ClientDeadlines {
  readonly unaryMs?: number;
  readonly commandAttempts?: number;
  readonly retryBackoffMs?: number;
}

export interface CreateAgentClientOptions {
  readonly binding: EpochBinding;
  readonly deadlines?: ClientDeadlines;
  readonly clientInstanceId?: string;
}

export interface HandshakeInput {
  readonly clientName: string;
  readonly clientVersion: string;
}

export interface HandshakeDto {
  readonly protocolVersion: number;
  readonly serverVersion: string;
  readonly serverInstanceId: string;
  readonly capabilities: readonly string[];
  readonly scopeId: string;
}

export interface HealthDto {
  readonly protocolVersion: number;
  readonly status: "serving";
}

export interface CommandReceiptDto {
  readonly commandId: string;
  readonly accepted: true;
}

export type RuntimePhaseDto =
  | "initializing"
  | "model-setup-required"
  | "ready"
  | "preparing"
  | "streaming"
  | "awaiting-approval"
  | "running-tool"
  | "cancelling"
  | "completed"
  | "failed-recoverable"
  | "failed-terminal";

export interface RuntimeSnapshotDto {
  readonly protocolVersion: number;
  readonly revision: bigint;
  readonly phase: RuntimePhaseDto;
  readonly sessionId: string | null;
  readonly turns: readonly TurnSnapshotDto[];
  readonly queuedFollowUps: readonly string[];
  readonly pendingApproval: ApprovalSnapshotDto | null;
  readonly lastError: string | null;
  readonly modelSelection: ModelSelectionDto;
  readonly route: RouteSummaryDto | null;
  readonly modelSetup: ModelSetupDto | null;
  readonly modelCatalog: ModelCatalogDto | null;
  readonly modelPolicy: ModelPolicyDto | null;
}

export interface RuntimeEventDto {
  readonly protocolVersion: number;
  readonly revision: bigint;
  readonly snapshot: RuntimeSnapshotDto;
}

export interface TurnSnapshotDto {
  readonly turnId: string;
  readonly userText: string;
  readonly assistantText: string;
  readonly status: "pending" | "running" | "completed" | "cancelled" | "failed";
  readonly steering: readonly string[];
  readonly activities: readonly ActivitySnapshotDto[];
}

export interface ActivitySnapshotDto {
  readonly stepId: string;
  readonly phase:
    | "preparing"
    | "streaming"
    | "awaiting-approval"
    | "running-tool"
    | "superseded"
    | "completed"
    | "failed";
  readonly route: RouteSummaryDto | null;
  readonly context: ContextSummaryDto | null;
  readonly assistantText: string;
  readonly usage: TokenUsageDto | null;
  readonly tools: readonly ToolActivityDto[];
  readonly outcome: StepOutcomeDto | null;
}

export interface ModelRouteDto {
  readonly provider: string;
  readonly model: string;
}

export interface ModelFallbackDto {
  readonly from: ModelRouteDto;
  readonly to: ModelRouteDto;
  readonly reason: string;
}

export interface RouteSummaryDto {
  readonly selected: ModelRouteDto;
  readonly reason: string;
  readonly fallbacks: readonly ModelFallbackDto[];
}

export interface ContextSummaryDto {
  readonly systemTokens: number;
  readonly contextTokens: number;
  readonly dropped: number;
}

export interface TokenUsageDto {
  readonly input: number;
  readonly output: number;
  readonly cacheRead: number;
}

export interface ToolResultDto {
  readonly invocationId: string;
  readonly ok: boolean;
  readonly output: string;
  readonly truncated: boolean;
  readonly durationMs: bigint;
  readonly error: string | null;
}

export interface ToolActivityDto {
  readonly callId: string;
  readonly name: string;
  readonly argumentsJson: string;
  readonly state: "requested" | "approved" | "running" | "completed" | "failed";
  readonly approvedBy: string | null;
  readonly result: ToolResultDto | null;
}

export interface StepOutcomeDto {
  readonly stopReason: StopReasonDto;
  readonly text: string;
  readonly toolCallIds: readonly string[];
  readonly escalated: boolean;
}

export interface StopReasonDto {
  readonly kind: "end-turn" | "tool-use" | "max-tokens" | "stop" | "error";
  readonly error: string | null;
}

export interface ToolPreviewDto {
  readonly tool: string;
  readonly action: string;
  readonly target: string;
  readonly impact: string;
  readonly risk: "read" | "write" | "execute" | "network";
  readonly sandbox: "workspace-restricted" | "host-unsandboxed" | "unknown";
  readonly workingDirectory: string | null;
  readonly requiredSecrets: readonly string[];
}

export interface ApprovalSnapshotDto {
  readonly requestId: string;
  readonly toolName: string;
  readonly argumentsJson: string;
  readonly stepId: string;
  readonly preview: ToolPreviewDto | null;
  readonly deadlineRemainingMs: bigint | null;
  readonly state: "pending" | "approved";
  readonly approvedBy: string | null;
}

export interface ManagedModelDto {
  readonly id: string;
  readonly displayName: string | null;
  readonly recommendationReason:
    | "installed-preferred"
    | "hardware-compatible"
    | "conservative-fallback"
    | null;
  readonly expectedDownloadBytes: bigint;
  readonly maxBytes: bigint;
  readonly minAvailableMemoryBytes: bigint;
  readonly sha256: string;
}

export interface ModelSetupDto {
  readonly source: "managed-preset" | "configured-path";
  readonly phase:
    | "required"
    | "waiting-for-lock"
    | "checking"
    | "downloading"
    | "verifying"
    | "installing"
    | "activating"
    | "ready"
    | "cancelled"
    | "failed";
  readonly managedModel: ManagedModelDto | null;
  readonly operationId: string | null;
  readonly downloadedBytes: bigint;
  readonly totalBytes: bigint | null;
  readonly failure: {
    readonly code: string;
    readonly message: string;
    readonly retryable: boolean;
  } | null;
  readonly allowedActions: readonly ("download" | "cancel" | "retry")[];
}

export interface ModelOptionDto {
  readonly route: ModelRouteDto;
  readonly tier: "control" | "worker" | "frontier";
  readonly locality: "local" | "remote";
  readonly contextWindow: number;
  readonly maxOutputTokens: number;
  readonly supportsTools: boolean;
  readonly supportsStreaming: boolean;
  readonly supportsJson: boolean;
  readonly supportsThinking: boolean;
  readonly selectableForWorker: boolean;
  readonly disabledReason: "not-worker-tier" | "tools-unsupported" | "streaming-unsupported" | null;
}

export interface ModelCatalogDto {
  readonly models: readonly ModelOptionDto[];
}

export interface ModelPolicyDto {
  readonly active: ModelSelectionDto;
  readonly pending: ModelSelectionDto | null;
}

export interface SubscribeEventsInput {
  readonly afterRevision: bigint;
  readonly signal?: AbortSignal;
}

export interface AgentClient {
  health(): Promise<HealthDto>;
  handshake(input: HandshakeInput): Promise<HandshakeDto>;
  enqueue(command: AgentCommand): Promise<CommandReceiptDto>;
  getSnapshot(): Promise<RuntimeSnapshotDto>;
  subscribeEvents(input: SubscribeEventsInput): AsyncIterable<RuntimeEventDto>;
  replaceBinding(binding: EpochBinding): void;
  close(): Promise<void>;
}
