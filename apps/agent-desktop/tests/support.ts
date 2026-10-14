import type { RuntimeSnapshotDto } from "@lyonbrown/agent-client";
export function snapshot(overrides: Partial<RuntimeSnapshotDto> = {}): RuntimeSnapshotDto {
  return {
    protocolVersion: 1,
    revision: 1n,
    phase: "ready",
    sessionId: "session",
    turns: [],
    queuedFollowUps: [],
    pendingApproval: null,
    lastError: null,
    modelSelection: { kind: "auto" },
    route: null,
    modelSetup: null,
    decisionModel: null,
    modelCatalog: null,
    modelPolicy: null,
    ...overrides,
  };
}
