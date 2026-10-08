import type { NativeControlPlaneProvider } from "@lyonbrown/local-control-plane";

import { startProductionAgentd as startProductionAgentdInternal } from "@lyonbrown/agentd/internal/bootstrap";
import { createProductionAgentdRuntime as createProductionAgentdRuntimeInternal } from "@lyonbrown/agentd/internal/production-runtime";

export {
  createProductionDecisionService,
  type ProductionDecisionService,
} from "@lyonbrown/agentd/internal/decision-service";

export interface StartProductionAgentdOptions {
  readonly workspace: string;
  readonly homeDirectory: string;
  readonly effectiveConfigFingerprint: string;
  readonly expectedScopeId: string;
  readonly nativeProvider: NativeControlPlaneProvider;
}

export interface RunningProductionAgentd {
  readonly address: string;
  readonly port: number;
  readonly scopeId: string;
  close(): Promise<void>;
}

export function startProductionAgentd(
  options: StartProductionAgentdOptions,
): Promise<RunningProductionAgentd> {
  return startProductionAgentdInternal({
    ...options,
    createRuntime: createProductionAgentdRuntimeInternal,
  });
}
