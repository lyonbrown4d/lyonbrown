import type { UiModelState } from "./types.ts";

export function getFreshCurrentServerInstanceId(state: UiModelState): string | null {
  const { remote } = state;
  if (
    remote.connection !== "ready" ||
    !remote.snapshotFresh ||
    remote.serverInstanceId === null ||
    remote.snapshot?.serverInstanceId !== remote.serverInstanceId
  ) {
    return null;
  }
  return remote.serverInstanceId;
}
