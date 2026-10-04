import { createHash, randomUUID } from "node:crypto";
import { createReadStream } from "node:fs";
import { lstat, mkdir, rename, rm, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

import { createModelDownloader } from "node-llama-cpp";
import lockfile from "proper-lockfile";

import { MODEL_DOWNLOAD_URI } from "./catalog.js";
import type {
  DownloadArtifact,
  DownloadProgress,
  InstallLockLease,
  LocalModelIo,
  ModelPreset,
} from "./contracts.js";

interface ModelDownloaderApi {
  createModelDownloader(options: {
    modelUri: string;
    dirPath: string;
    fileName: string;
    showCliProgress: false;
    skipExisting: false;
    deleteTempFileOnCancel: true;
    parallelDownloads: number;
    onProgress: (status: { downloadedSize: number; totalSize: number }) => void;
  }): Promise<{ download(options?: { signal?: AbortSignal }): Promise<string> }>;
}

interface NativeArtifact {
  readonly operationId: string;
  readonly stagingDirectory: string;
  readonly filePath: string;
}

export class NodeModelIo implements LocalModelIo {
  private readonly artifacts = new WeakMap<DownloadArtifact, NativeArtifact>();

  constructor(
    private readonly modelsDirectory: string,
    private readonly api: ModelDownloaderApi = { createModelDownloader },
  ) {}

  async inspectInstalled(preset: ModelPreset, signal: AbortSignal): Promise<boolean> {
    const destination = this.destination(preset);
    try {
      const metadata = await lstat(destination);
      if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > preset.maxBytes) {
        return false;
      }
      return (await this.digest(destination, signal)) === preset.sha256;
    } catch (error) {
      if (isNotFound(error)) return false;
      throw new Error("model_inspection_failed");
    }
  }

  async acquireInstallLock(preset: ModelPreset, signal: AbortSignal): Promise<InstallLockLease> {
    await mkdir(this.modelsDirectory, { recursive: true });
    const target = join(this.modelsDirectory, `.${preset.id}.install`);
    await writeFile(target, "", { flag: "a" });
    for (;;) {
      this.throwIfAborted(signal);
      try {
        const release = await lockfile.lock(target, {
          realpath: false,
          retries: 0,
          stale: 30_000,
          update: 10_000,
        });
        let released = false;
        return {
          async release(): Promise<void> {
            if (released) return;
            released = true;
            await release();
          },
        };
      } catch (error) {
        if (!isLockContended(error)) throw new Error("model_install_lock_failed");
        try {
          await delay(50, undefined, { signal });
        } catch {
          throw new Error("model_download_cancelled");
        }
      }
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
    await mkdir(this.modelsDirectory, { recursive: true });
    const stagingDirectory = join(this.modelsDirectory, `.staging-${randomUUID()}`);
    await mkdir(stagingDirectory, { recursive: false });
    try {
      const downloader = await this.api.createModelDownloader({
        modelUri: MODEL_DOWNLOAD_URI,
        dirPath: stagingDirectory,
        fileName: preset.filename,
        showCliProgress: false,
        skipExisting: false,
        deleteTempFileOnCancel: true,
        parallelDownloads: 4,
        onProgress: ({ downloadedSize, totalSize }) => {
          request.onProgress({ downloadedBytes: downloadedSize, totalBytes: totalSize });
        },
      });
      const filePath = resolve(await downloader.download({ signal: request.signal }));
      const expectedPath = resolve(stagingDirectory, preset.filename);
      if (filePath !== expectedPath) throw new Error("model_download_target_invalid");
      const artifact = Object.freeze({ operationId: request.operationId, token: Symbol() });
      this.artifacts.set(artifact, {
        operationId: request.operationId,
        stagingDirectory,
        filePath,
      });
      return artifact;
    } catch (error) {
      await rm(stagingDirectory, { recursive: true, force: true });
      throw error;
    }
  }

  async verify(
    artifact: DownloadArtifact,
    preset: ModelPreset,
    signal: AbortSignal,
  ): Promise<number> {
    const native = this.requireArtifact(artifact);
    const metadata = await lstat(native.filePath);
    if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > preset.maxBytes) {
      throw new Error("model_integrity_failed");
    }
    const digest = await this.digest(native.filePath, signal);
    if (digest !== preset.sha256) throw new Error("model_integrity_failed");
    return metadata.size;
  }

  async activate(artifact: DownloadArtifact, preset: ModelPreset): Promise<void> {
    const native = this.requireArtifact(artifact);
    await rename(native.filePath, this.destination(preset));
  }

  async cleanup(artifact: DownloadArtifact | undefined): Promise<void> {
    if (artifact === undefined) return;
    const native = this.artifacts.get(artifact);
    if (native === undefined) return;
    this.artifacts.delete(artifact);
    await rm(native.stagingDirectory, { recursive: true, force: true });
  }

  private destination(preset: ModelPreset): string {
    return join(this.modelsDirectory, preset.filename);
  }

  private requireArtifact(artifact: DownloadArtifact): NativeArtifact {
    const native = this.artifacts.get(artifact);
    if (native === undefined || native.operationId !== artifact.operationId) {
      throw new Error("model_artifact_invalid");
    }
    return native;
  }

  private async digest(path: string, signal?: AbortSignal): Promise<string> {
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(path)) {
      if (signal?.aborted) throw new Error("model_download_cancelled");
      hash.update(chunk as Buffer);
    }
    return hash.digest("hex");
  }

  private throwIfAborted(signal: AbortSignal): void {
    if (signal.aborted) throw new Error("model_download_cancelled");
  }
}

const isNotFound = (error: unknown): boolean =>
  typeof error === "object" && error !== null && "code" in error && error.code === "ENOENT";

const isLockContended = (error: unknown): boolean =>
  typeof error === "object" && error !== null && "code" in error && error.code === "ELOCKED";
