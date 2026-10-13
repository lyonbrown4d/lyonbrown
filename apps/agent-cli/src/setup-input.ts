import { Key, matchesKey } from "@earendil-works/pi-tui";

import type { AgentTuiFrame } from "@lyonbrown/agent-cli/internal/controller";

export type SetupKeyIntent = "download" | "retry" | "cancel" | "exit" | "blocked";

export function setupKeyIntent(data: string, frame: AgentTuiFrame): SetupKeyIntent | null {
  if (frame.overlay !== null) return null;
  const setup = frame.snapshot?.modelSetup;
  if (setup === null || setup === undefined || setup.phase === "ready") return null;

  if (matchesKey(data, "q")) return "exit";
  if (
    matchesKey(data, Key.escape) &&
    (setup.phase === "required" || setup.phase === "failed" || setup.phase === "cancelled")
  ) {
    return "exit";
  }

  const fresh = frame.ui.remote.connection === "ready" && frame.ui.remote.snapshotFresh;
  if (!fresh) return "blocked";
  if (
    setup.allowedActions.includes("cancel") &&
    setup.operationId !== null &&
    (matchesKey(data, "c") || matchesKey(data, Key.escape))
  ) {
    return "cancel";
  }
  if (
    setup.allowedActions.includes("retry") &&
    (matchesKey(data, Key.enter) || matchesKey(data, "d") || matchesKey(data, "r"))
  ) {
    return "retry";
  }
  if (
    setup.allowedActions.includes("download") &&
    (matchesKey(data, Key.enter) || matchesKey(data, "d"))
  ) {
    return "download";
  }
  return "blocked";
}
