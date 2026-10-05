import { launchError } from "./errors.ts";
import type { AgentLaunchDeadlines, CreateAgentLauncherOptions } from "./types.ts";

const DEFAULT_STARTUP_MS = 30_000;
const DEFAULT_POLL_INTERVAL_MS = 25;
const DEFAULT_CLEANUP_MS = 1_000;

export interface NormalizedConfiguration {
  readonly startupMs: number;
  readonly pollIntervalMs: number;
  readonly cleanupMs: number;
}

function positiveInteger(value: number, name: string): number {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw launchError("validation", "INVALID_CONFIGURATION", `${name} must be positive`);
  }
  return value;
}

function requiredText(value: string, name: string): string {
  if (value.trim().length === 0 || value.includes("\0")) {
    throw launchError("validation", "INVALID_CONFIGURATION", `${name} is invalid`);
  }
  return value;
}

export function validateOptions(options: CreateAgentLauncherOptions): NormalizedConfiguration {
  requiredText(options.workspace, "workspace");
  requiredText(options.effectiveConfigFingerprint, "effectiveConfigFingerprint");
  requiredText(options.client.name, "client.name");
  requiredText(options.client.version, "client.version");
  const deadlines: AgentLaunchDeadlines = options.deadlines ?? {};
  const startupMs = positiveInteger(deadlines.startupMs ?? DEFAULT_STARTUP_MS, "startupMs");
  const pollIntervalMs = positiveInteger(
    deadlines.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
    "pollIntervalMs",
  );
  const cleanupMs = positiveInteger(deadlines.cleanupMs ?? DEFAULT_CLEANUP_MS, "cleanupMs");
  return Object.freeze({
    startupMs,
    pollIntervalMs: Math.min(startupMs, pollIntervalMs),
    cleanupMs,
  });
}
