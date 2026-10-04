import type { LocalModelIo, LlamaEngineAdapter, LlamaLease } from "./internal/contracts.js";
import { createLocalModelManager } from "./internal/manager.js";
import type { LocalModelLease, LocalModelService } from "./types.js";

export interface SecureModelDownloadArtifact {
  readonly operationId: string;
  readonly token: symbol;
}

export interface SecureModelDownloadProgress {
  readonly downloadedBytes: number;
  readonly totalBytes: number | null;
}

export interface SecureModelInstallLock {
  release(): Promise<void>;
}

export interface SecureVerifiedModelAnchor {
  load(signal: AbortSignal): Promise<LocalModelLease>;
  release(): Promise<void>;
}

export interface SecureLocalModelCapability {
  inspectInstalled(signal: AbortSignal): Promise<boolean>;
  acquireInstallLock(signal: AbortSignal): Promise<SecureModelInstallLock>;
  download(request: {
    readonly operationId: string;
    readonly signal: AbortSignal;
    readonly onProgress: (progress: SecureModelDownloadProgress) => void;
  }): Promise<SecureModelDownloadArtifact>;
  verify(artifact: SecureModelDownloadArtifact, signal: AbortSignal): Promise<number>;
  activate(artifact: SecureModelDownloadArtifact): Promise<void>;
  cleanup(artifact: SecureModelDownloadArtifact | undefined): Promise<void>;
  acquireVerifiedModelAnchor(signal: AbortSignal): Promise<SecureVerifiedModelAnchor>;
}

export const createLocalModelService = (
  capability: SecureLocalModelCapability,
): LocalModelService => {
  if (!isSecureCapability(capability)) throw new Error("secure_model_capability_required");
  return createLocalModelManager(adaptIo(capability), adaptLoader(capability));
};

const adaptIo = (capability: SecureLocalModelCapability): LocalModelIo => ({
  inspectInstalled: (_preset, signal) => capability.inspectInstalled(signal),
  acquireInstallLock: (_preset, signal) => capability.acquireInstallLock(signal),
  download: (_preset, request) => capability.download(request),
  verify: (artifact, _preset, signal) => capability.verify(artifact, signal),
  activate: (artifact) => capability.activate(artifact),
  cleanup: (artifact) => capability.cleanup(artifact),
});

const adaptLoader = (capability: SecureLocalModelCapability): LlamaEngineAdapter => ({
  async loadDefault(signal: AbortSignal): Promise<LlamaLease> {
    const anchor = await capability.acquireVerifiedModelAnchor(signal);
    let lease: LocalModelLease;
    try {
      lease = await anchor.load(signal);
    } catch (error) {
      await releaseQuietly(anchor);
      throw error;
    }
    try {
      await anchor.release();
      return lease;
    } catch {
      await disposeQuietly(lease);
      throw new Error("secure_model_anchor_release_failed");
    }
  },
});

const releaseQuietly = async (anchor: SecureVerifiedModelAnchor): Promise<void> => {
  try {
    await anchor.release();
  } catch {
    // The manager maps all provider failures to a typed public result.
  }
};

const disposeQuietly = async (lease: LocalModelLease): Promise<void> => {
  try {
    await lease.dispose();
  } catch {
    // Never replace a capability boundary failure with private native details.
  }
};

const isSecureCapability = (value: unknown): value is SecureLocalModelCapability =>
  typeof value === "object" &&
  value !== null &&
  [
    "inspectInstalled",
    "acquireInstallLock",
    "download",
    "verify",
    "activate",
    "cleanup",
    "acquireVerifiedModelAnchor",
  ].every((key) => typeof Reflect.get(value, key) === "function");
