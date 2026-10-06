import type { AgentRuntime } from "@lyonbrown/agent-runtime";
import type { EventStore } from "@lyonbrown/event-store";
import type { LocalModelLease, LocalModelService } from "@lyonbrown/local-model";

import {
  collectCleanupFailure,
  disposeActivationResources,
  drainActivationTask,
} from "@lyonbrown/agentd/internal/cleanup";
import type { CreateModelAwareRuntimeOptions } from "@lyonbrown/agentd/internal/model-aware-runtime-types";
import { createProductionRuntime } from "@lyonbrown/agentd/internal/production-kernel";

interface ModelActivationOptions {
  readonly modelService: LocalModelService;
  readonly eventStore: EventStore;
  readonly sessionId: string;
  readonly createRuntime: CreateModelAwareRuntimeOptions["createRuntime"];
  readonly isClosed: () => boolean;
  readonly onActivating: () => void;
  readonly onFailed: () => void;
  readonly onReady: (runtime: AgentRuntime, lease: LocalModelLease) => void;
}

export class ModelActivationController {
  private task: Promise<void> | undefined;
  private taskError: unknown;

  constructor(private readonly options: ModelActivationOptions) {}

  begin(): void {
    if (this.task !== undefined || this.options.isClosed()) return;
    this.options.onActivating();
    const task = this.activate();
    this.task = task;
    void task.then(
      () => {
        this.clear(task);
      },
      (error: unknown) => {
        this.taskError = error;
        this.clear(task);
      },
    );
  }

  retry(): void {
    const pending = this.task;
    if (pending === undefined) {
      this.begin();
      return;
    }
    void pending.then(
      () => {
        this.begin();
      },
      () => {
        this.begin();
      },
    );
  }

  async drain(failures: unknown[]): Promise<void> {
    const pending = this.task;
    await collectCleanupFailure(() => drainActivationTask(pending), failures);
    if (pending === undefined && this.taskError !== undefined) failures.push(this.taskError);
  }

  private clear(task: Promise<void>): void {
    if (this.task === task) this.task = undefined;
  }

  private async activate(): Promise<void> {
    let result: Awaited<ReturnType<LocalModelService["load"]>>;
    try {
      result = await this.options.modelService.load();
    } catch {
      if (!this.options.isClosed()) this.options.onFailed();
      return;
    }
    if (this.options.isClosed()) {
      if (result.kind === "ready") await disposeActivationResources(undefined, result.lease);
      return;
    }
    if (result.kind !== "ready") {
      this.options.onFailed();
      return;
    }
    const create = this.options.createRuntime ?? createProductionRuntime;
    let runtime: AgentRuntime;
    try {
      runtime = await create(
        result.lease,
        result.model.id,
        this.options.eventStore,
        this.options.sessionId,
      );
    } catch {
      if (!this.options.isClosed()) this.options.onFailed();
      await disposeActivationResources(undefined, result.lease);
      return;
    }
    if (this.options.isClosed()) {
      await disposeActivationResources(runtime, result.lease);
      return;
    }
    try {
      this.options.onReady(runtime, result.lease);
    } catch {
      this.options.onFailed();
      await disposeActivationResources(runtime, result.lease);
    }
  }
}
