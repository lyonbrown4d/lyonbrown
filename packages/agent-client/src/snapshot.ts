import { PROTOCOL_VERSION, type RuntimeEvent, type RuntimeSnapshot } from "@lyonbrown/protocol";

import { clientError } from "@lyonbrown/agent-client/internal/errors";
import { mapApproval, mapTurn } from "@lyonbrown/agent-client/internal/snapshot-activity";
import {
  mapCatalog,
  mapModelSetup,
  mapPolicy,
  mapRouteSummary,
  mapSelection,
} from "@lyonbrown/agent-client/internal/snapshot-model";
import { runtimePhase } from "@lyonbrown/agent-client/internal/snapshot-scalars";
import type { RuntimeEventDto, RuntimeSnapshotDto } from "@lyonbrown/agent-client/internal/types";
import {
  protocolId,
  protocolText,
  requireNonNegativeRevision,
  requiredField,
} from "@lyonbrown/agent-client/internal/validation";

export function mapSnapshot(wire: RuntimeSnapshot): RuntimeSnapshotDto {
  if (wire.protocolVersion !== PROTOCOL_VERSION) {
    throw clientError(
      "protocol",
      "INCOMPATIBLE_VERSION",
      "snapshot protocol version is incompatible",
    );
  }
  const snapshot: RuntimeSnapshotDto = {
    protocolVersion: wire.protocolVersion,
    revision: requireNonNegativeRevision("snapshot.revision", wire.revision),
    phase: runtimePhase(wire.phase),
    sessionId:
      wire.sessionId === undefined ? null : protocolId("snapshot.session_id", wire.sessionId),
    turns: wire.turns.map(mapTurn),
    queuedFollowUps: wire.queuedFollowUps.map((text) =>
      protocolText("snapshot.queued_follow_up", text),
    ),
    pendingApproval: mapApproval(wire.pendingApproval),
    lastError:
      wire.lastError === undefined ? null : protocolText("snapshot.last_error", wire.lastError),
    modelSelection: mapSelection(wire.modelSelection),
    route: mapRouteSummary(wire.route),
    modelSetup: mapModelSetup(wire.modelSetup),
    modelCatalog: mapCatalog(wire.modelCatalog),
    modelPolicy: mapPolicy(wire.modelPolicy),
  };
  if (
    snapshot.modelPolicy !== null &&
    JSON.stringify(snapshot.modelPolicy.active) !== JSON.stringify(snapshot.modelSelection)
  ) {
    throw clientError(
      "protocol",
      "INVALID_SNAPSHOT",
      "model policy conflicts with model selection",
    );
  }
  return snapshot;
}

export function mapEvent(wire: RuntimeEvent): RuntimeEventDto {
  if (wire.protocolVersion !== PROTOCOL_VERSION) {
    throw clientError("protocol", "INCOMPATIBLE_VERSION", "event protocol version is incompatible");
  }
  const outerRevision = requireNonNegativeRevision("event.revision", wire.revision);
  const snapshot = mapSnapshot(requiredField("event.snapshot", wire.snapshot));
  if (outerRevision !== snapshot.revision) {
    throw clientError(
      "protocol",
      "REVISION_MISMATCH",
      "event revision does not match snapshot revision",
      { eventRevision: outerRevision, snapshotRevision: snapshot.revision },
    );
  }
  return { protocolVersion: wire.protocolVersion, revision: outerRevision, snapshot };
}
