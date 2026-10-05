import { performance } from "node:perf_hooks";
import { setTimeout as delay } from "node:timers/promises";
import { createAgentLauncher } from "./launcher.ts";
import {
  createProductionAgentClientFactory,
  createProductionDiscoveryClient,
} from "./production-discovery.ts";
import { NodeProcessSpawner, type AgentdProcessConfiguration } from "./production-spawner.ts";
import type { AgentLauncher, Clock, CreateAgentLauncherOptions, Sleeper } from "./types.ts";

export interface CreateProductionAgentLauncherOptions
  extends Omit<CreateAgentLauncherOptions, "ports"> {
  readonly homeDirectory: string;
  readonly agentd: AgentdProcessConfiguration;
}

export class MonotonicClock implements Clock {
  public now(): number {
    return performance.now();
  }
}

export class AbortableSleeper implements Sleeper {
  public async sleep(durationMs: number, signal?: AbortSignal): Promise<void> {
    if (signal === undefined) {
      await delay(durationMs);
      return;
    }
    await delay(durationMs, undefined, { signal });
  }
}

export function createProductionAgentLauncher(
  options: CreateProductionAgentLauncherOptions,
): AgentLauncher {
  return createAgentLauncher({
    nativeProvider: options.nativeProvider,
    workspace: options.workspace,
    effectiveConfigFingerprint: options.effectiveConfigFingerprint,
    client: options.client,
    ...(options.deadlines === undefined ? {} : { deadlines: options.deadlines }),
    ports: {
      discovery: createProductionDiscoveryClient({
        nativeProvider: options.nativeProvider,
        homeDirectory: options.homeDirectory,
        effectiveConfigFingerprint: options.effectiveConfigFingerprint,
      }),
      spawner: new NodeProcessSpawner(options.agentd),
      clients: createProductionAgentClientFactory(),
      clock: new MonotonicClock(),
      sleeper: new AbortableSleeper(),
    },
  });
}

export type { AgentdProcessConfiguration } from "./production-spawner.ts";
