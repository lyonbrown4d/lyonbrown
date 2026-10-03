import type { DiscoveryDescriptor } from "./descriptor.js";

export interface EpochBinding {
  readonly endpoint: string;
  readonly pid: number;
  readonly scopeId: string;
  readonly serverInstanceId: string;
  readonly token: DiscoveryDescriptor["token"];
}

export class StaleServerEpochError extends Error {
  public override readonly name = "StaleServerEpochError";
  public readonly code = "STALE_SERVER_EPOCH";

  public constructor(
    public readonly expectedServerInstanceId: string,
    public readonly actualServerInstanceId: string,
  ) {
    super("command is bound to a stale server epoch");
  }
}

export function bindDiscoveredEpoch(descriptor: DiscoveryDescriptor): EpochBinding {
  return Object.freeze({
    endpoint: descriptor.endpoint,
    pid: descriptor.pid,
    scopeId: descriptor.scopeId,
    serverInstanceId: descriptor.serverInstanceId,
    token: descriptor.token,
  });
}

function fence(binding: EpochBinding, currentServerInstanceId: string): void {
  if (binding.serverInstanceId !== currentServerInstanceId) {
    throw new StaleServerEpochError(binding.serverInstanceId, currentServerInstanceId);
  }
}

export function fenceEpochAtEnqueue(binding: EpochBinding, currentServerInstanceId: string): void {
  fence(binding, currentServerInstanceId);
}

export function fenceEpochAtExecution(
  binding: EpochBinding,
  currentServerInstanceId: string,
): void {
  fence(binding, currentServerInstanceId);
}
