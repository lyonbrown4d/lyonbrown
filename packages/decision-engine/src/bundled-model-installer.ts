import { setTimeout as delay } from "node:timers/promises";

import lockfile from "proper-lockfile";

import { BUNDLED_DECISION_MODEL } from "@lyonbrown/decision-engine/internal/bundled-model-catalog";
import {
  downloadDecisionModelBundle,
  type DecisionModelDownloadProgress,
} from "@lyonbrown/decision-engine/internal/bundled-model-download";
import { verifyDecisionModelBundle } from "@lyonbrown/decision-engine/internal/bundled-model-verification";
import type {
  DecisionModelBundleIdentity,
  DecisionModelBundleInstallLock,
  DecisionModelBundleStoragePort,
} from "@lyonbrown/decision-engine/internal/model-bundle-storage";

export type BundledDecisionModelInspection =
  | { readonly kind: "missing" }
  | { readonly kind: "present"; readonly modelPath: string }
  | { readonly kind: "corrupted" };

export type BundledDecisionModelInstallResult =
  | { readonly kind: "installed"; readonly modelPath: string }
  | { readonly kind: "already_installed"; readonly modelPath: string };

export type ProcessLockAcquirer = (
  target: string,
  signal: AbortSignal,
) => Promise<() => Promise<void>>;

export interface InstallBundledDecisionModelOptions {
  readonly operationId: string;
  readonly signal: AbortSignal;
  readonly fetcher?: typeof globalThis.fetch;
  readonly acquireProcessLock?: ProcessLockAcquirer;
  readonly onProgress?: (progress: DecisionModelDownloadProgress) => void;
}

export async function inspectBundledDecisionModel(
  storage: DecisionModelBundleStoragePort,
  signal: AbortSignal,
): Promise<BundledDecisionModelInspection> {
  return inspectDecisionModelBundle(storage, BUNDLED_DECISION_MODEL, signal);
}

export async function inspectDecisionModelBundle(
  storage: DecisionModelBundleStoragePort,
  model: DecisionModelBundleIdentity,
  signal: AbortSignal,
): Promise<BundledDecisionModelInspection> {
  try {
    const result = await storage.withInstalledBundle(model, signal, async (modelPath) => {
      await verifyDecisionModelBundle(modelPath, model, signal);
      return modelPath;
    });
    return result.kind === "missing"
      ? { kind: "missing" }
      : { kind: "present", modelPath: result.value };
  } catch (error) {
    if (signal.aborted) throw new Error("decision_model_install_cancelled", { cause: error });
    return { kind: "corrupted" };
  }
}

export async function installBundledDecisionModel(
  storage: DecisionModelBundleStoragePort,
  options: InstallBundledDecisionModelOptions,
): Promise<BundledDecisionModelInstallResult> {
  return installDecisionModelBundle(storage, BUNDLED_DECISION_MODEL, options);
}

export async function installDecisionModelBundle(
  storage: DecisionModelBundleStoragePort,
  model: DecisionModelBundleIdentity,
  options: InstallBundledDecisionModelOptions,
): Promise<BundledDecisionModelInstallResult> {
  const existing = await inspectDecisionModelBundle(storage, model, options.signal);
  if (existing.kind === "present") {
    return { kind: "already_installed", modelPath: existing.modelPath };
  }

  const anchor = await storage.acquireInstallLockAnchor(model, options.signal);
  let releaseProcessLock: (() => Promise<void>) | undefined;
  try {
    releaseProcessLock = await anchor.withLockTarget((target) =>
      (options.acquireProcessLock ?? acquireProcessLock)(target, options.signal),
    );
    const afterLock = await inspectDecisionModelBundle(storage, model, options.signal);
    if (afterLock.kind === "present") {
      return { kind: "already_installed", modelPath: afterLock.modelPath };
    }

    const staging = await storage.createStagingBundle(model, options.operationId, options.signal);
    try {
      await staging.withDirectoryPath(async (directoryPath) => {
        await downloadDecisionModelBundle(
          directoryPath,
          model,
          options.signal,
          options.fetcher ?? globalThis.fetch,
          options.onProgress ?? (() => undefined),
        );
        await verifyDecisionModelBundle(directoryPath, model, options.signal);
      });
      await staging.activate();
    } catch (error) {
      await staging.cleanup();
      throw normalizeInstallError(error, options.signal);
    }
    await staging.cleanup();

    const installed = await inspectDecisionModelBundle(storage, model, options.signal);
    if (installed.kind !== "present") throw new Error("decision_model_activation_failed");
    return { kind: "installed", modelPath: installed.modelPath };
  } finally {
    try {
      await releaseProcessLock?.();
    } finally {
      await releaseAnchorQuietly(anchor);
    }
  }
}

async function acquireProcessLock(
  target: string,
  signal: AbortSignal,
): Promise<() => Promise<void>> {
  for (;;) {
    throwIfAborted(signal);
    try {
      return await lockfile.lock(target, {
        realpath: false,
        retries: 0,
        stale: 30_000,
        update: 10_000,
      });
    } catch (error) {
      if (!isLockContended(error)) throw new Error("decision_model_install_lock_failed");
      await delay(50, undefined, { signal });
    }
  }
}

async function releaseAnchorQuietly(anchor: DecisionModelBundleInstallLock): Promise<void> {
  try {
    await anchor.release();
  } catch {
    // The install result remains authoritative; native storage logs its own privileged failure.
  }
}

function normalizeInstallError(error: unknown, signal: AbortSignal): Error {
  if (signal.aborted) return new Error("decision_model_install_cancelled", { cause: error });
  if (error instanceof Error && error.message === "decision_model_integrity_failed") return error;
  if (error instanceof Error && error.name === "AbortError" && error.cause instanceof Error) {
    return error.cause;
  }
  return error instanceof Error ? error : new Error("decision_model_install_failed");
}

function isLockContended(error: unknown): boolean {
  return typeof error === "object" && error !== null && "code" in error && error.code === "ELOCKED";
}

function throwIfAborted(signal: AbortSignal): void {
  if (signal.aborted) throw new Error("decision_model_install_cancelled");
}
