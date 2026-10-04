import type { RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import type { ModelSetupState, RemoteSnapshot } from "@lyonbrown/ui-model";

const PHASES = {
  required: "Required",
  "waiting-for-lock": "WaitingForLock",
  checking: "Checking",
  downloading: "Downloading",
  verifying: "Verifying",
  installing: "Installing",
  activating: "Activating",
  ready: "Ready",
  cancelled: "Cancelled",
  failed: "Failed",
} as const;

export function projectRemoteSnapshot(
  serverInstanceId: string,
  snapshot: RuntimeSnapshotDto,
): RemoteSnapshot {
  return {
    serverInstanceId,
    revision: snapshot.revision,
    sessionId: snapshot.sessionId ?? "pending-session",
    modelSetup: projectModelSetup(snapshot),
    activities: snapshot.turns.flatMap((turn) =>
      turn.activities.map((activity) => ({
        turnId: turn.turnId,
        stepId: activity.stepId,
        kind: activity.phase,
      })),
    ),
  };
}

function projectModelSetup(snapshot: RuntimeSnapshotDto): ModelSetupState | null {
  const setup = snapshot.modelSetup;
  if (setup === null) return null;
  const phase = PHASES[setup.phase];
  if (phase === "Required" || phase === "Ready") return { phase, operationId: null };
  if (setup.operationId === null) return null;
  return { phase, operationId: setup.operationId };
}
