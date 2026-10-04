import { randomUUID } from "node:crypto";
import {
  ControlPlaneError,
  SecretToken,
  deriveScope,
  type NativeControlPlaneProvider,
} from "@lyonbrown/local-control-plane";
import type { RuntimeHost } from "./runtime-host.js";
import { startAgentdServer, type RunningAgentdServer } from "./server.js";

export interface ProductionAgentdOptions {
  readonly workspace: string;
  readonly effectiveConfigFingerprint: string;
  readonly nativeProvider?: NativeControlPlaneProvider;
  readonly host?: RuntimeHost;
}

export async function startProductionAgentd(
  options: ProductionAgentdOptions,
): Promise<RunningAgentdServer> {
  if (options.nativeProvider === undefined) {
    throw new ControlPlaneError("NATIVE_SECURITY_PROVIDER_REQUIRED");
  }
  if (options.host === undefined) throw new Error("runtime host composition is required");
  const scope = await deriveScope(
    options.workspace,
    options.effectiveConfigFingerprint,
    options.nativeProvider,
  );
  return startAgentdServer({
    host: options.host,
    token: SecretToken.generate(),
    serverInstanceId: randomUUID(),
    scopeFingerprint: scope.scopeId,
  });
}
