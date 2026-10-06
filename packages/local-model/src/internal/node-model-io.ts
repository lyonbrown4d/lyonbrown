import { setTimeout as delay } from "node:timers/promises";

import lockfile from "proper-lockfile";

import type {
  NativeInstallLockAnchor,
  NativeModelStagingFile,
  NativeModelStorageCapability,
} from "../native-storage.ts";
import { MODEL_DOWNLOAD_URI } from "./catalog.ts";
import type {
  DownloadArtifact,
  DownloadProgress,
  InstallLockLease,
  LocalModelIo,
  ModelPreset,
} from "./contracts.ts";
import { verifyModelFile } from "./file-verification.ts";
import { downloadModelFile, type ModelFileDownloadRequest } from "./model-download.ts";
import { DEFAULT_MODEL_IDENTITY } from "./model-identity.ts";

export interface ModelFileDownloader {
  downloadModelFile(request: ModelFileDownloadRequest): Promise<void>;
}

export type ModelFileVerifier = typeof verifyModelFile;
export type ProcessLockAcquirer = typeof acquireProcessLock;

interface NativeArtifact {
  readonly operationId: string;
  readonly staging: NativeModelStagingFile;
}

export class NodeModelIo implements LocalModelIo {
  private readonly artifacts = new WeakMap<DownloadArtifact, NativeArtifact>();

  constructor(
    private readonly storage: NativeModelStorageCapability,
    private readonly downloader: ModelFileDownloader = {
      downloadModelFile,
    },
    private readonly verifyFile: ModelFileVerifier = verifyModelFile,
    private readonly lockProcess: ProcessLockAcquirer = acquireProcessLock,
  ) {}

  async inspectInstalled(preset: ModelPreset, signal: AbortSignal): Promise<boolean> {
    let verified = false;
    const result = await this.storage.withInstalledFile(
      DEFAULT_MODEL_IDENTITY,
      signal,
      async (filePath) => {
        await this.verifyFile(filePath, preset, signal);
        verified = true;
        return true;
      },
    );
    return result.kind === "present" && result.value && verified;
  }

  async acquireInstallLock(preset: ModelPreset, signal: AbortSignal): Promise<InstallLockLease> {
    void preset;
    throwIfAborted(signal);
    const anchor = await this.storage.acquireInstallLockAnchor(DEFAULT_MODEL_IDENTITY, signal);
    try {
      const releaseLock = await anchor.withLockTarget((target) => this.lockProcess(target, signal));
      return combineLockReleases(releaseLock, anchor);
    } catch (error) {
      await releaseAnchorQuietly(anchor);
      throw error;
    }
  }

  async download(
    preset: ModelPreset,
    request: {
      operationId: string;
      signal: AbortSignal;
      onProgress: (progress: DownloadProgress) => void;
    },
  ): Promise<DownloadArtifact> {
    const staging = await this.storage.createStagingFile(
      DEFAULT_MODEL_IDENTITY,
      request.operationId,
      request.signal,
    );
    try {
      await staging.withFilePath((filePath) => this.downloadToPath(preset, request, filePath));
      const artifact = Object.freeze({ operationId: request.operationId, token: Symbol() });
      this.artifacts.set(artifact, { operationId: request.operationId, staging });
      return artifact;
    } catch (error) {
      await staging.cleanup();
      throw error;
    }
  }

  async verify(
    artifact: DownloadArtifact,
    preset: ModelPreset,
    signal: AbortSignal,
  ): Promise<number> {
    const native = this.requireArtifact(artifact);
    let verifiedBytes: number | undefined;
    const returned = await native.staging.withFilePath(async (filePath) => {
      verifiedBytes = await this.verifyFile(filePath, preset, signal);
      return verifiedBytes;
    });
    if (verifiedBytes === undefined || returned !== verifiedBytes) {
      throw new Error("model_integrity_failed");
    }
    return verifiedBytes;
  }

  async activate(artifact: DownloadArtifact): Promise<void> {
    await this.requireArtifact(artifact).staging.activate();
  }

  async cleanup(artifact: DownloadArtifact | undefined): Promise<void> {
    if (artifact === undefined) return;
    const native = this.artifacts.get(artifact);
    if (native === undefined) return;
    this.artifacts.delete(artifact);
    await native.staging.cleanup();
  }

  private async downloadToPath(
    preset: ModelPreset,
    request: {
      readonly signal: AbortSignal;
      readonly onProgress: (progress: DownloadProgress) => void;
    },
    expectedPath: string,
  ): Promise<void> {
    const controller = new AbortController();
    const forwardAbort = () => {
      controller.abort(request.signal.reason);
    };
    if (request.signal.aborted) forwardAbort();
    else request.signal.addEventListener("abort", forwardAbort, { once: true });
    let boundaryFailure = false;
    let previousDownloadedBytes = 0;
    let previousTotalBytes = 0;
    try {
      await this.downloader.downloadModelFile({
        source: MODEL_DOWNLOAD_URI,
        targetPath: expectedPath,
        maxBytes: preset.maxBytes,
        signal: controller.signal,
        onProgress: ({ downloadedBytes, totalBytes }) => {
          if (
            boundaryFailure ||
            !validDownloadProgress(
              downloadedBytes,
              totalBytes,
              previousDownloadedBytes,
              previousTotalBytes,
              preset.maxBytes,
            )
          ) {
            boundaryFailure = true;
            controller.abort(new Error("model_download_limits_exceeded"));
            return;
          }
          previousDownloadedBytes = downloadedBytes;
          previousTotalBytes = totalBytes;
          request.onProgress({ downloadedBytes, totalBytes });
        },
      });
      throwIfAborted(controller.signal);
      throwIfAborted(request.signal);
    } finally {
      request.signal.removeEventListener("abort", forwardAbort);
    }
  }

  private requireArtifact(artifact: DownloadArtifact): NativeArtifact {
    const native = this.artifacts.get(artifact);
    if (native === undefined || native.operationId !== artifact.operationId) {
      throw new Error("model_artifact_invalid");
    }
    return native;
  }
}

const acquireProcessLock = async (
  target: string,
  signal: AbortSignal,
): Promise<() => Promise<void>> => {
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
      if (!isLockContended(error)) throw new Error("model_install_lock_failed");
      try {
        await delay(50, undefined, { signal });
      } catch {
        throw new Error("model_download_cancelled");
      }
    }
  }
};

const combineLockReleases = (
  releaseLock: () => Promise<void>,
  anchor: NativeInstallLockAnchor,
): InstallLockLease => {
  let released = false;
  return {
    async release(): Promise<void> {
      if (released) return;
      released = true;
      try {
        await releaseLock();
      } finally {
        await anchor.release();
      }
    },
  };
};

const releaseAnchorQuietly = async (anchor: NativeInstallLockAnchor): Promise<void> => {
  try {
    await anchor.release();
  } catch {
    // The public manager sanitizes privileged storage failures.
  }
};

const throwIfAborted = (signal: AbortSignal): void => {
  if (signal.aborted) throw new Error("model_download_cancelled");
};

const isLockContended = (error: unknown): boolean =>
  typeof error === "object" && error !== null && "code" in error && error.code === "ELOCKED";

const validDownloadProgress = (
  downloadedBytes: number,
  totalBytes: number,
  previousDownloadedBytes: number,
  previousTotalBytes: number,
  maxBytes: number,
): boolean =>
  Number.isSafeInteger(downloadedBytes) &&
  Number.isSafeInteger(totalBytes) &&
  downloadedBytes >= previousDownloadedBytes &&
  totalBytes >= previousTotalBytes &&
  downloadedBytes >= 0 &&
  totalBytes >= downloadedBytes &&
  downloadedBytes <= maxBytes &&
  totalBytes <= maxBytes;
