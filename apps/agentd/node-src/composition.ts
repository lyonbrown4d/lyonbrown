import type { NativeControlPlaneProvider } from "@lyonbrown/local-control-plane";

import { startProductionAgentd as startProductionAgentdInternal } from "./bootstrap.ts";
import { createProductionAgentdRuntime as createProductionAgentdRuntimeInternal } from "./production-runtime.ts";

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
