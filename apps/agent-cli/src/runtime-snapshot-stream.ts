import type { AgentClient, RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import { from, map, type Observable } from "rxjs";

export function observeRuntimeSnapshots(
  client: AgentClient,
  afterRevision: bigint,
  signal: AbortSignal,
): Observable<RuntimeSnapshotDto> {
  return from(client.subscribeEvents({ afterRevision, signal })).pipe(
    map((event) => event.snapshot),
  );
}
