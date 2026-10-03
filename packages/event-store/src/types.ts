export interface EventToAppend {
  readonly eventId: string;
  readonly type: string;
  readonly occurredAt?: string;
  readonly payload: unknown;
}

export interface StoredEvent {
  readonly eventId: string;
  readonly sessionId: string;
  readonly revision: bigint;
  readonly type: string;
  readonly occurredAt: string;
  readonly payload: unknown;
}

export interface LoadOptions {
  readonly afterRevision: bigint;
  readonly limit: number;
}

export interface ListSessionsOptions {
  readonly limit: number;
  readonly offset?: number;
}

export interface SessionMetadata {
  readonly sessionId: string;
  readonly currentRevision: bigint;
  readonly createdAt: string;
  readonly updatedAt: string;
}

export interface StoreDiagnostics {
  readonly journalMode: string;
  readonly schemaVersion: number;
  readonly synchronous: number;
}

export interface PathSecurityVerifier {
  verifyDataDirectory(canonicalDataDirectory: string): void;
  verifyDatabaseFile(databasePath: string): void;
}

export type OpenEventStoreOptions =
  | {
      readonly dataDirectory: string;
      readonly pathVerifier?: PathSecurityVerifier;
      readonly busyTimeoutMs?: number;
      readonly testOnlyMemory?: never;
    }
  | {
      readonly testOnlyMemory: true;
      readonly dataDirectory?: never;
      readonly pathVerifier?: never;
      readonly busyTimeoutMs?: number;
    };

export interface EventStore {
  append(
    sessionId: string,
    expectedRevision: bigint,
    events: readonly EventToAppend[],
  ): Promise<readonly StoredEvent[]>;
  load(sessionId: string, options: LoadOptions): Promise<readonly StoredEvent[]>;
  currentRevision(sessionId: string): Promise<bigint>;
  listSessions(options: ListSessionsOptions): Promise<readonly SessionMetadata[]>;
  diagnostics(): StoreDiagnostics;
  close(): void;
}

export type RecoveredToolCallStatus = "completed" | "failed" | "cancelled" | "outcome-unknown";

export interface RecoveredToolCall {
  readonly toolCallId: string;
  readonly toolName?: string;
  readonly status: RecoveredToolCallStatus;
  readonly startedEventId: string;
  readonly terminalEventId?: string;
}
