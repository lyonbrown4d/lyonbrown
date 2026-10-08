import { assertSmokeSnapshotHealthy } from "@lyonbrown/workspace/internal/smoke-real-gguf-assertions";
import type {
  RealGgufSmokeFlowOptions,
  RealGgufSmokePort,
  SmokeSnapshot,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-flow";
import { waitForCondition } from "@lyonbrown/workspace/internal/smoke-real-gguf-lib";

export function waitSmokeSnapshot(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
  label: string,
  accept: (snapshot: SmokeSnapshot) => boolean,
  observe?: (snapshot: SmokeSnapshot) => void,
): Promise<SmokeSnapshot> {
  return waitForCondition({
    label,
    timeoutMs: options.phaseTimeoutMs,
    intervalMs: options.pollIntervalMs,
    read: async () => {
      const snapshot = await port.readSnapshot();
      assertSmokeSnapshotHealthy(snapshot);
      return snapshot;
    },
    accept,
    ...(observe === undefined ? {} : { observe }),
  });
}
