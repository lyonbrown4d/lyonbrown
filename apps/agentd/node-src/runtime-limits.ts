import { LOCAL_MODEL_MAX_MESSAGE_BYTES } from "@lyonbrown/local-model";
import type { KernelResourceBudget } from "@lyonbrown/kernel-pi";

export const KERNEL_MAX_INPUT_BYTES = 262_144;
export const MAX_MESSAGE_INPUT_BYTES = LOCAL_MODEL_MAX_MESSAGE_BYTES;
export const MAX_SESSION_TURNS = 32;
export const MAX_SESSION_USER_BYTES = 4 * 1_024 * 1_024;
export const MAX_SESSION_STEERING_PER_TURN = 16;
export const MAX_SESSION_STEERING_BYTES = 256 * 1_024;
export const MAX_QUEUED_FOLLOW_UPS = 8;
export const MAX_QUEUED_FOLLOW_UP_BYTES = 512 * 1_024;
export const ACTIVATION_SHUTDOWN_TIMEOUT_MS = 5_000;

export const PRODUCTION_KERNEL_BUDGET: KernelResourceBudget = Object.freeze({
  maxInputBytes: KERNEL_MAX_INPUT_BYTES,
  maxTurns: 8,
  maxSteps: 16,
  maxOutputBytes: 262_144,
  maxToolResultBytes: 65_536,
  maxValueDepth: 16,
  maxValueStringBytes: 65_536,
  maxCollectionEntries: 1_024,
  maxValueNodes: 8_192,
  toolTimeoutMs: 30_000,
  maxSecretCount: 1,
  maxSecretBytes: 1,
});
