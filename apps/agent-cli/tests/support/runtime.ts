import type { RuntimeSnapshotDto } from "@lyonbrown/agent-client";

export function runtimeSnapshot(overrides: Partial<RuntimeSnapshotDto> = {}): RuntimeSnapshotDto {
  return {
    protocolVersion: 1,
    revision: 1n,
    phase: "ready",
    sessionId: "session-1",
    turns: [],
    queuedFollowUps: [],
    pendingApproval: null,
    lastError: null,
    modelSelection: { kind: "auto" },
    route: null,
    modelSetup: null,
    modelCatalog: null,
    modelPolicy: null,
    ...overrides,
  };
}
