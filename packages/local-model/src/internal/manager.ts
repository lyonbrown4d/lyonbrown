import { randomUUID } from "node:crypto";

import { DEFAULT_LOCAL_MODEL } from "../public-model.js";
import type {
  CancelModelDownloadResult,
  LocalModelLoadResult,
  LocalModelService,
  LocalModelStatus,
  StartModelDownloadResult,
} from "../types.js";
import { DEFAULT_PRESET } from "./catalog.js";
import type { DownloadArtifact, LocalModelIo, LlamaEngineAdapter } from "./contracts.js";
import { cloneStatus, statusFor, validProgress } from "./status.js";

interface ActiveOperation {
  readonly id: string;
  readonly controller: AbortController;
}

interface ActiveLoad {
  readonly controller: AbortController;
  readonly task: Promise<LocalModelLoadResult>;
}

const DEFAULT_SHUTDOWN_TIMEOUT_MS = 5_000;

export const createLocalModelManager = (
  io: LocalModelIo,
  engine: LlamaEngineAdapter,
  operationId: () => string = randomUUID,
  shutdownTimeoutMs = DEFAULT_SHUTDOWN_TIMEOUT_MS,
): LocalModelService => new LocalModelManager(io, engine, operationId, shutdownTimeoutMs);

class LocalModelManager implements LocalModelService {
  private current: LocalModelStatus;
  private active: ActiveOperation | undefined;
  private readonly inspectionController = new AbortController();
  private readonly inspectionTask: Promise<void>;
  private readonly operationTasks = new Set<Promise<void>>();
  private readonly loadTasks = new Set<ActiveLoad>();
  private disposeTask: Promise<void> | undefined;
  private disposed = false;
  private readonly listeners = new Set<(status: LocalModelStatus) => void>();

  constructor(
    private readonly io: LocalModelIo,
    private readonly engine: LlamaEngineAdapter,
    private readonly operationId: () => string,
    private readonly shutdownTimeoutMs: number,
  ) {
    this.current = statusFor("checking");
    this.inspectionTask = this.inspectInitialCache();
  }

  status(): LocalModelStatus {
    return cloneStatus(this.current);
  }

  subscribe(listener: (status: LocalModelStatus) => void): () => void {
    if (this.disposed) return () => undefined;
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  startDownload(): StartModelDownloadResult {
    if (this.disposed) return { kind: "disposed" };
    if (this.current.phase === "checking") return { kind: "checking" };
    if (this.current.phase === "ready") return { kind: "already-ready" };
    if (this.active !== undefined) return { kind: "in-progress", operationId: this.active.id };
    const id = this.operationId();
    const controller = new AbortController();
    this.active = { id, controller };
    this.publish(statusFor("waiting-for-lock", id, 0, DEFAULT_PRESET.expectedDownloadBytes));
    const task = this.runDownload(id, controller);
    this.operationTasks.add(task);
    void task.then(
      () => this.operationTasks.delete(task),
      () => this.operationTasks.delete(task),
    );
    return { kind: "started", operationId: id };
  }

  cancelDownload(operationId: string): CancelModelDownloadResult {
    const active = this.active;
    if (active === undefined) return { kind: "no-active-operation" };
    if (active.id !== operationId) {
      return { kind: "operation-mismatch", activeOperationId: active.id };
    }
    if (this.current.phase === "installing") return { kind: "too-late" };
    active.controller.abort(new Error("model_download_cancelled"));
    return { kind: "cancelled" };
  }

  async load(): Promise<LocalModelLoadResult> {
    if (this.disposed) return { kind: "unavailable", code: "service_disposed" } as const;
    if (this.current.phase === "checking") return { kind: "checking" } as const;
    if (this.current.phase !== "ready") return { kind: "setup-required" } as const;
    const controller = new AbortController();
    const task = this.loadReady(controller.signal);
    const activeLoad = { controller, task };
    this.loadTasks.add(activeLoad);
    try {
      return await task;
    } finally {
      this.loadTasks.delete(activeLoad);
    }
  }

  dispose(): Promise<void> {
    if (this.disposeTask !== undefined) return this.disposeTask;
    this.disposed = true;
    this.inspectionController.abort(new Error("local_model_service_disposed"));
    this.active?.controller.abort(new Error("local_model_service_disposed"));
    for (const load of this.loadTasks) {
      load.controller.abort(new Error("local_model_service_disposed"));
    }
    this.disposeTask = this.joinBackgroundTasks();
    return this.disposeTask;
  }

  private async inspectInitialCache(): Promise<void> {
    try {
      const installed = await this.io.inspectInstalled(
        DEFAULT_PRESET,
        this.inspectionController.signal,
      );
      this.publish(statusFor(installed ? "ready" : "setup-required"));
    } catch {
      if (!this.inspectionController.signal.aborted) {
        this.publish(
          statusFor("failed", null, 0, null, {
            code: "cache_inspection_failed",
            retryable: true,
          }),
        );
      }
    }
  }

  private async joinBackgroundTasks(): Promise<void> {
    const tasks: Promise<unknown>[] = [
      this.inspectionTask,
      ...this.operationTasks,
      ...[...this.loadTasks].map(({ task }) => task),
    ];
    await waitForTasks(tasks, this.shutdownTimeoutMs);
    this.listeners.clear();
  }

  private async loadReady(signal: AbortSignal): Promise<LocalModelLoadResult> {
    try {
      const lease = await this.engine.loadDefault(signal);
      if (this.disposed) {
        try {
          await lease.dispose();
        } catch {
          // A late native lease remains private; disposal failures are redacted.
        }
        return { kind: "unavailable", code: "service_disposed" };
      }
      return { kind: "ready", model: DEFAULT_LOCAL_MODEL, lease };
    } catch {
      return {
        kind: "unavailable",
        code: this.disposed || signal.aborted ? "service_disposed" : "native_runtime_unavailable",
      };
    }
  }

  private async runDownload(id: string, controller: AbortController): Promise<void> {
    let artifact: DownloadArtifact | undefined;
    let releaseLock: (() => Promise<void>) | undefined;
    try {
      const lock = await this.io.acquireInstallLock(DEFAULT_PRESET, controller.signal);
      releaseLock = () => lock.release();
      this.throwIfCancelled(controller.signal);
      if (await this.io.inspectInstalled(DEFAULT_PRESET, controller.signal)) {
        if (!this.isActive(id)) return;
        this.active = undefined;
        this.publish(statusFor("ready"));
        return;
      }
      this.publishFor(id, statusFor("downloading", id, 0, DEFAULT_PRESET.expectedDownloadBytes));
      artifact = await this.io.download(DEFAULT_PRESET, {
        operationId: id,
        signal: controller.signal,
        onProgress: (progress) => {
          this.onProgress(id, progress.downloadedBytes, progress.totalBytes);
        },
      });
      this.throwIfCancelled(controller.signal);
      this.publishFor(id, statusFor("verifying", id));
      const bytes = await this.io.verify(artifact, DEFAULT_PRESET, controller.signal);
      this.throwIfCancelled(controller.signal);
      this.publishFor(id, statusFor("installing", id, bytes, bytes));
      if (!this.isActive(id)) return;
      await this.io.activate(artifact, DEFAULT_PRESET);
      if (!this.isActive(id)) return;
      this.active = undefined;
      this.publish(statusFor("ready", null, bytes, bytes));
    } catch {
      if (!this.isActive(id)) return;
      const cancelled = controller.signal.aborted;
      this.active = undefined;
      this.publish(
        cancelled
          ? statusFor("cancelled", id)
          : statusFor("failed", id, 0, null, { code: "download_failed", retryable: true }),
      );
    } finally {
      try {
        await this.io.cleanup(artifact);
      } catch {
        // Cleanup cannot change an already-fenced public operation result.
      }
      try {
        await releaseLock?.();
      } catch {
        // Lock release failure is private and cannot rewrite an operation result.
      }
    }
  }

  private onProgress(id: string, downloadedBytes: number, totalBytes: number | null): void {
    if (!validProgress(downloadedBytes, totalBytes)) return;
    this.publishFor(id, statusFor("downloading", id, downloadedBytes, totalBytes));
  }

  private publishFor(id: string, status: LocalModelStatus): void {
    if (this.isActive(id)) this.publish(status);
  }

  private isActive(id: string): boolean {
    return this.active?.id === id;
  }

  private throwIfCancelled(signal: AbortSignal): void {
    if (signal.aborted) throw new Error("model_download_cancelled");
  }

  private publish(status: LocalModelStatus): void {
    if (this.disposed) return;
    this.current = status;
    for (const listener of this.listeners) {
      try {
        listener(cloneStatus(status));
      } catch {
        // One observer must not affect the daemon-owned operation.
      }
    }
  }
}

const waitForTasks = async (
  tasks: readonly Promise<unknown>[],
  timeoutMs: number,
): Promise<void> => {
  await new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, Math.max(0, timeoutMs));
    void Promise.allSettled(tasks).then(() => {
      clearTimeout(timer);
      resolve();
    });
  });
};
