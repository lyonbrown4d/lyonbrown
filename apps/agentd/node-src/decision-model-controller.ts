import { randomUUID } from "node:crypto";

import { createNativeModelBundleStorageCapability } from "@lyonbrown/control-plane-native";
import {
  inspectBundledDecisionModel,
  readDecisionModelStorageConfiguration,
  type BundledDecisionModelInspection,
} from "@lyonbrown/decision-engine/composition";

import {
  runDecisionInstallProcess,
  type DecisionInstallProgress,
} from "@lyonbrown/agentd/internal/decision-install-process";
import {
  initialDecisionModelStatus,
  type DecisionModelControlPort,
  type DecisionModelStatus,
} from "@lyonbrown/agentd/internal/decision-model-state";
import type { RuntimeResult } from "@lyonbrown/agentd/internal/runtime-host";

export interface DecisionModelControllerDependencies {
  readonly inspect: (root: string, signal: AbortSignal) => Promise<BundledDecisionModelInspection>;
  readonly install: (
    root: string,
    operationId: string,
    signal: AbortSignal,
    progress: (value: DecisionInstallProgress) => void,
  ) => Promise<void>;
}

const DEFAULT_DEPENDENCIES: DecisionModelControllerDependencies = {
  inspect: (root, signal) =>
    inspectBundledDecisionModel(createNativeModelBundleStorageCapability(root), signal),
  install: runDecisionInstallProcess,
};

export interface DecisionModelController extends DecisionModelControlPort {
  dispose(): Promise<void>;
}

export function createDecisionModelController(
  environment: Readonly<NodeJS.ProcessEnv>,
  dependencies: DecisionModelControllerDependencies = DEFAULT_DEPENDENCIES,
): DecisionModelController {
  return new Controller(environment, dependencies);
}

class Controller implements DecisionModelController {
  private current: DecisionModelStatus;
  private readonly root: string | null;
  private readonly listeners = new Set<(status: DecisionModelStatus) => void>();
  private readonly lifecycle = new AbortController();
  private operation: AbortController | undefined;
  private task: Promise<void>;
  private closed = false;

  constructor(
    environment: Readonly<NodeJS.ProcessEnv>,
    private readonly dependencies: DecisionModelControllerDependencies,
  ) {
    const configuration = readDecisionModelStorageConfiguration(environment);
    this.root = configuration.kind === "configured" ? configuration.rootDirectory : null;
    this.current = initialDecisionModelStatus(this.root === null ? "unavailable" : "checking");
    this.task = this.root === null ? Promise.resolve() : this.inspect(this.root);
  }

  status(): DecisionModelStatus {
    return this.current;
  }

  subscribe(listener: (status: DecisionModelStatus) => void): () => void {
    if (!this.closed) this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  start(modelId: string): RuntimeResult {
    if (this.closed) return failure("RUNTIME_STOPPED");
    if (modelId !== this.current.modelId)
      return {
        ok: false,
        error: {
          code: "INVALID_MODEL_ID",
          requestedModelId: modelId,
          configuredModelId: this.current.modelId,
        },
      };
    if (
      this.root === null ||
      this.operation !== undefined ||
      !this.current.allowedActions.some((a) => a === "download" || a === "retry")
    )
      return failure("MODEL_DOWNLOAD_UNAVAILABLE");
    const abort = new AbortController();
    this.operation = abort;
    const operationId = randomUUID();
    this.update({
      phase: "checking",
      operationId,
      downloadedBytes: 0,
      failure: null,
      allowedActions: ["cancel"],
    });
    this.task = this.install(this.root, operationId, abort);
    return { ok: true };
  }

  cancel(operationId: string): RuntimeResult {
    if (this.closed) return failure("RUNTIME_STOPPED");
    if (this.operation === undefined || this.current.operationId === null)
      return failure("NO_ACTIVE_MODEL_DOWNLOAD");
    if (operationId !== this.current.operationId)
      return {
        ok: false,
        error: {
          code: "MODEL_DOWNLOAD_OPERATION_MISMATCH",
          expectedOperationId: this.current.operationId,
          receivedOperationId: operationId,
        },
      };
    if (!this.current.allowedActions.includes("cancel"))
      return failure("MODEL_DOWNLOAD_CANCELLATION_TOO_LATE");
    this.operation.abort();
    this.update({ allowedActions: [] });
    return { ok: true };
  }

  async dispose(): Promise<void> {
    this.closed = true;
    this.listeners.clear();
    this.lifecycle.abort();
    this.operation?.abort();
    await this.task;
  }

  private async inspect(root: string): Promise<void> {
    try {
      const result = await this.dependencies.inspect(
        root,
        AbortSignal.any([this.lifecycle.signal, AbortSignal.timeout(120_000)]),
      );
      if (this.closed) return;
      if (result.kind === "present") this.update({ phase: "configured", allowedActions: [] });
      else if (result.kind === "missing")
        this.update({ phase: "required", allowedActions: ["download"] });
      else this.failed("decision_model_corrupted");
    } catch {
      if (!this.closed) this.failed("decision_model_inspection_failed");
    }
  }

  private async install(root: string, id: string, abort: AbortController): Promise<void> {
    try {
      await this.dependencies.install(
        root,
        id,
        AbortSignal.any([abort.signal, this.lifecycle.signal, AbortSignal.timeout(60 * 60_000)]),
        (progress) => {
          if (this.closed || abort.signal.aborted) return;
          this.update({
            phase: progress.phase,
            ...(progress.downloadedBytes === undefined
              ? {}
              : { downloadedBytes: progress.downloadedBytes }),
            allowedActions: progress.phase === "installing" ? [] : ["cancel"],
          });
        },
      );
      if (!this.closed)
        this.update({
          phase: "configured",
          operationId: null,
          downloadedBytes: this.current.totalBytes,
          allowedActions: [],
        });
    } catch {
      if (this.closed) return;
      if (abort.signal.aborted)
        this.update({ phase: "cancelled", operationId: null, allowedActions: ["retry"] });
      else this.failed("decision_model_install_failed");
    } finally {
      this.operation = undefined;
    }
  }

  private failed(code: string): void {
    this.update({
      phase: "failed",
      operationId: null,
      failure: { code, message: "Decision model setup failed", retryable: true },
      allowedActions: ["retry"],
    });
  }

  private update(patch: Partial<DecisionModelStatus>): void {
    this.current = Object.freeze({
      ...this.current,
      ...patch,
      allowedActions: Object.freeze([...(patch.allowedActions ?? this.current.allowedActions)]),
    });
    for (const listener of this.listeners) listener(this.current);
  }
}

function failure(
  code:
    | "RUNTIME_STOPPED"
    | "MODEL_DOWNLOAD_UNAVAILABLE"
    | "NO_ACTIVE_MODEL_DOWNLOAD"
    | "MODEL_DOWNLOAD_CANCELLATION_TOO_LATE",
): RuntimeResult {
  return { ok: false, error: { code } };
}
