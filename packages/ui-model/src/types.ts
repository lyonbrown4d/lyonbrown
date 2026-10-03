export type ConnectionState =
  | "disconnected"
  | "connecting"
  | "synchronizing"
  | "ready"
  | "stale"
  | "unavailable";

export type ModelSetupPhase =
  | "WaitingForLock"
  | "Checking"
  | "Downloading"
  | "Verifying"
  | "Installing"
  | "Activating"
  | "Ready"
  | "Cancelled"
  | "Failed";

export interface ModelSetupState {
  readonly operationId: string;
  readonly phase: ModelSetupPhase;
}

export interface ActivityItemDto {
  readonly turnId: string;
  readonly stepId: string;
  readonly kind: string;
}

export interface RemoteSnapshot {
  readonly serverInstanceId: string;
  readonly revision: bigint;
  readonly sessionId: string;
  readonly modelSetup: ModelSetupState | null;
  readonly activities: readonly ActivityItemDto[];
}

export interface RemoteState {
  readonly connection: ConnectionState;
  readonly serverInstanceId: string | null;
  readonly snapshot: RemoteSnapshot | null;
  readonly snapshotFresh: boolean;
  readonly unavailableReason: string | null;
}

export interface ModelDownloadIntent {
  readonly operationId: string;
  readonly modelId: string;
}

export interface ApprovalRequestDto {
  readonly requestId: string;
  readonly summary: string;
}

export type ApprovalStatus =
  | "pending"
  | "denying"
  | "timed-out"
  | "errored"
  | "failed"
  | "disconnected";

export interface ApprovalState {
  readonly request: ApprovalRequestDto;
  readonly status: ApprovalStatus;
}

export interface ActivitySelection {
  readonly turnId: string;
  readonly stepId: string;
}

export type NarrowViewMode = "transcript" | "activity";

export interface InspectorState {
  readonly selection: ActivitySelection | null;
  readonly followLatest: boolean;
  readonly narrowViewMode: NarrowViewMode;
}

export type Locale = "en-US" | "zh-CN";

export const THEME_IDS = [
  "carbon-mint",
  "ink-violet",
  "slate-blue",
  "hearth-teal",
  "sandstone-espresso",
  "graphite-coral",
] as const;

export type ThemeId = (typeof THEME_IDS)[number];

export interface PreferencesState {
  readonly locale: Locale;
  readonly themeId: ThemeId;
}

export interface UiModelState {
  readonly remote: RemoteState;
  readonly modelDownloadIntent: ModelDownloadIntent | null;
  readonly approval: ApprovalState | null;
  readonly inspector: InspectorState;
  readonly preferences: PreferencesState;
}

export type UiModelAction =
  | { readonly type: "clientOpened" }
  | { readonly type: "connectRequested" }
  | { readonly type: "clientDisconnected" }
  | { readonly type: "connectionLost"; readonly reason: string }
  | { readonly type: "connectionUnavailable"; readonly reason: string }
  | { readonly type: "handshakeSucceeded"; readonly serverInstanceId: string }
  | { readonly type: "snapshotReceived"; readonly snapshot: RemoteSnapshot }
  | {
      readonly type: "modelDownloadRequested";
      readonly operationId: string;
      readonly modelId: string;
    }
  | { readonly type: "modelDownloadIntentAcknowledged"; readonly operationId: string }
  | { readonly type: "approvalRequested"; readonly request: ApprovalRequestDto }
  | { readonly type: "approvalDenied" }
  | { readonly type: "approvalTimedOut" }
  | { readonly type: "approvalErrored" }
  | { readonly type: "approvalFailed" }
  | { readonly type: "approvalResolved"; readonly requestId: string }
  | { readonly type: "activitySelected"; readonly selection: ActivitySelection | null }
  | { readonly type: "followLatestChanged"; readonly value: boolean }
  | { readonly type: "narrowViewModeChanged"; readonly mode: NarrowViewMode }
  | { readonly type: "localeChanged"; readonly locale: Locale }
  | { readonly type: "themeChanged"; readonly themeId: ThemeId };

export type MutationDisableReason =
  | "disconnected"
  | "connecting"
  | "synchronizing"
  | "stale-snapshot"
  | "unavailable"
  | "wrong-epoch"
  | "model-setup-required";

export type MutationAvailability =
  | { readonly enabled: true }
  | { readonly enabled: false; readonly reason: MutationDisableReason };

export type ModelSetupCancellation =
  | { readonly allowed: true; readonly operationId: string }
  | { readonly allowed: false; readonly reason: "no-operation" | "too-late" | "terminal" };

export type ApprovalDisposition =
  | { readonly kind: "none" }
  | { readonly kind: "decision-required"; readonly requestId: string }
  | {
      readonly kind: "deny-intent";
      readonly requestId: string;
      readonly reason: "user" | "timeout" | "error" | "failure" | "disconnected";
    };

export type InputPriority = "approval" | "model-setup" | "composer";
