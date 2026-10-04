import type { NativeModelIdentity } from "@lyonbrown/local-model/composition";

import type { OwnedHandle } from "./handles.js";
import type { SharedFileLock } from "./model-files.js";
import { identityKey } from "./model-identity.js";

const PUBLIC_STORAGE_ERRORS = new Set([
  "native_model_identity_invalid",
  "native_model_operation_invalid",
  "native_model_directory_invalid",
  "model_download_cancelled",
]);

export const child = (directory: string, name: string): string => `${directory}\\${name}`;

export function throwIfAborted(signal: AbortSignal): void {
  if (signal.aborted) throw new Error("model_download_cancelled");
}

export function installedName(identity: NativeModelIdentity): string {
  return `model-${identityKey(identity)}.gguf`;
}

export function closeQuietly(handle: OwnedHandle | undefined): void {
  try {
    handle?.close();
  } catch {
    // A prior security failure remains authoritative.
  }
}

export function releaseQuietly(lock: SharedFileLock | undefined): void {
  try {
    lock?.release();
  } catch {
    // A prior security failure remains authoritative.
  }
}

export function storageFailure(error: unknown): Error {
  if (error instanceof Error && PUBLIC_STORAGE_ERRORS.has(error.message)) return error;
  return new Error("native_model_storage_failed");
}

export function promiseFrom<T>(operation: () => T | Promise<T>): Promise<T> {
  try {
    return Promise.resolve(operation());
  } catch (error) {
    return Promise.reject(
      error instanceof Error ? error : new Error("native_model_storage_failed"),
    );
  }
}
