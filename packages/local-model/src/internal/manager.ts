import { randomUUID } from "node:crypto";

import type {
  CancelModelDownloadResult,
  LocalModelLoadResult,
  LocalModelService,
  LocalModelSelectionReason,
  LocalModelStatus,
  StartModelDownloadResult,
} from "@lyonbrown/local-model/internal/types";
import { waitForTasks } from "@lyonbrown/local-model/internal/internal/background-tasks";
import { CATALOG_FALLBACK_PRESET } from "@lyonbrown/local-model/internal/internal/catalog";
import type {
  DownloadArtifact,
  LocalModelIo,
  LlamaEngineAdapter,
  ModelPreset,
} from "@lyonbrown/local-model/internal/internal/contracts";
import {
  createDeterministicSelectionOptions,
  type ModelSelectionOptions,
  selectInitialModel,
} from "@lyonbrown/local-model/internal/internal/initial-model-selection";
import { loadSelectedModel } from "@lyonbrown/local-model/internal/internal/model-loader";
import {
  cloneStatus,
  statusFor,
  validProgress,
} from "@lyonbrown/local-model/internal/internal/status";

interface ActiveOperation {
  readonly id: string;
  readonly controller: AbortController;
  invalidBoundaryInput: boolean;
  downloadedBytes: number;
  totalBytes: number | null;
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
  selectionOptions: Partial<ModelSelectionOptions> = createDeterministicSelectionOptions(
    CATALOG_FALLBACK_PRESET,
  ),
): LocalModelService => {
  const defaults = createDeterministicSelectionOptions(CATALOG_FALLBACK_PRESET);
  return new LocalModelManager(io, engine, operationId, shutdownTimeoutMs, {
    catalog: selectionOptions.catalog ?? defaults.catalog,
    selector: selectionOptions.selector ?? defaults.selector,
  });
};

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
  private selectedPreset: ModelPreset;
  private selectionReason: LocalModelSelectionReason | null = null;

  constructor(
    private readonly io: LocalModelIo,
    private readonly engine: LlamaEngineAdapter,
    private readonly operationId: () => string,
    private readonly shutdownTimeoutMs: number,
    private readonly selectionOptions: ModelSelectionOptions,
  ) {
    this.selectedPreset = selectionOptions.catalog[0] ?? CATALOG_FALLBACK_PRESET;
    this.current = statusFor(this.selectedPreset, null, "checking");
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
    this.active = {
      id,
      controller,
      invalidBoundaryInput: false,
      downloadedBytes: 0,
      totalBytes: null,
    };
    this.publish(
      statusFor(
        this.selectedPreset,
        this.selectionReason,
        "waiting-for-lock",
        id,
        0,
        this.selectedPreset.expectedDownloadBytes,
      ),
    );
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
      const selection = await selectInitialModel(
        this.io,
        this.selectionOptions,
        this.inspectionController.signal,
      );
      this.selectedPreset = selection.preset;
      this.selectionReason = selection.reason;
      this.publish(
        statusFor(
          this.selectedPreset,
          this.selectionReason,
          selection.installed ? "ready" : "setup-required",
        ),
      );
    } catch {
      if (!this.inspectionController.signal.aborted) {
        this.publish(
          statusFor(this.selectedPreset, this.selectionReason, "failed", null, 0, null, {
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
    return loadSelectedModel(this.engine, this.selectedPreset, signal, () => this.disposed);
  }

  private async runDownload(id: string, controller: AbortController): Promise<void> {
    const preset = this.selectedPreset;
    let artifact: DownloadArtifact | undefined;
    let releaseLock: (() => Promise<void>) | undefined;
    try {
      const lock = await this.io.acquireInstallLock(preset, controller.signal);
      releaseLock = () => lock.release();
      this.throwIfCancelled(controller.signal);
      if (await this.io.inspectInstalled(preset, controller.signal)) {
        if (!this.isActive(id)) return;
        this.active = undefined;
        this.publish(statusFor(preset, this.selectionReason, "ready"));
        return;
      }
      this.publishFor(
        id,
        statusFor(preset, this.selectionReason, "downloading", id, 0, preset.expectedDownloadBytes),
      );
      artifact = await this.io.download(preset, {
        operationId: id,
        signal: controller.signal,
        onProgress: (progress) => {
          this.onProgress(id, progress.downloadedBytes, progress.totalBytes);
        },
      });
      this.throwIfCancelled(controller.signal);
      this.publishFor(id, statusFor(preset, this.selectionReason, "verifying", id));
      const bytes = await this.io.verify(artifact, preset, controller.signal);
      if (!validVerifiedBytes(bytes, preset.maxBytes)) throw new Error("model_integrity_failed");
      this.throwIfCancelled(controller.signal);
      this.publishFor(id, statusFor(preset, this.selectionReason, "installing", id, bytes, bytes));
      if (!this.isActive(id)) return;
      await this.io.activate(artifact, preset);
      if (!this.isActive(id)) return;
      this.active = undefined;
      this.publish(statusFor(preset, this.selectionReason, "ready", null, bytes, bytes));
    } catch {
      const active = this.active;
      if (active?.id !== id) return;
      const cancelled = controller.signal.aborted && !active.invalidBoundaryInput;
      this.active = undefined;
      this.publish(
        cancelled
          ? statusFor(preset, this.selectionReason, "cancelled", id)
          : statusFor(preset, this.selectionReason, "failed", id, 0, null, {
              code: "download_failed",
              retryable: true,
            }),
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
    const active = this.active;
    if (active?.id !== id) return;
    if (
      !validProgress(
        downloadedBytes,
        totalBytes,
        active.downloadedBytes,
        active.totalBytes,
        this.selectedPreset.maxBytes,
      )
    ) {
      active.invalidBoundaryInput = true;
      active.controller.abort(new Error("model_download_limits_exceeded"));
      return;
    }
    active.downloadedBytes = downloadedBytes;
    active.totalBytes = totalBytes;
    this.publishFor(
      id,
      statusFor(
        this.selectedPreset,
        this.selectionReason,
        "downloading",
        id,
        downloadedBytes,
        totalBytes,
      ),
    );
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

const validVerifiedBytes = (bytes: number, maxBytes: number): boolean =>
  Number.isSafeInteger(bytes) && bytes >= 0 && bytes <= maxBytes;
