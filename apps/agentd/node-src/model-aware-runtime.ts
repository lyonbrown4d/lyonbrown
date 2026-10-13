import { asSessionId } from "@lyonbrown/agent-domain";
import type { AgentRuntime } from "@lyonbrown/agent-runtime";
import type { LocalModelLease, LocalModelStatus } from "@lyonbrown/local-model";
import {
  ModelSelection,
  ModelCatalog,
  ModelLocality,
  ModelOption,
  ModelPolicy,
  ModelTier,
  PROTOCOL_VERSION,
  RouteSummary,
  RuntimeEvent,
  RuntimeSnapshot,
} from "@lyonbrown/protocol";

import {
  latestRuntimeFeed,
  type RuntimeFeed,
} from "@lyonbrown/agentd/internal/latest-runtime-feed";
import { closeModelAwareResources } from "@lyonbrown/agentd/internal/model-aware-close";
import { projectDecisionModel } from "@lyonbrown/agentd/internal/decision-model-projection";
import { executeModelDownloadCommand } from "@lyonbrown/agentd/internal/model-download-commands";
import { failedRuntimeResult } from "@lyonbrown/agentd/internal/model-download-results";
import { ModelLeaseOwnership } from "@lyonbrown/agentd/internal/model-lease-ownership";
import type {
  CreateModelAwareRuntimeOptions,
  ModelAwareRuntime,
} from "@lyonbrown/agentd/internal/model-aware-runtime-types";
import { ModelActivationController } from "@lyonbrown/agentd/internal/model-activation";
import {
  projectModelSetup,
  projectSetupRuntimePhase,
  type ActivationPhase,
} from "@lyonbrown/agentd/internal/model-setup-projection";
import { createRuntimeHostAdapter } from "@lyonbrown/agentd/internal/runtime-adapter";
import {
  assertProtocolRevision,
  projectRuntimeSnapshot,
} from "@lyonbrown/agentd/internal/runtime-adapter-projection";
import { runtimeCommandWithinBudget } from "@lyonbrown/agentd/internal/runtime-command-budget";
import { RuntimeCommandSerialQueue } from "@lyonbrown/agentd/internal/runtime-command-serial-queue";
import type {
  RuntimeCommand,
  RuntimeHost,
  RuntimeResult,
} from "@lyonbrown/agentd/internal/runtime-host";
import { shouldRotateSession } from "@lyonbrown/agentd/internal/runtime-session-policy";
import { rotateRuntimeSession } from "@lyonbrown/agentd/internal/runtime-session-rotation";
import { nextTransportRevision } from "@lyonbrown/agentd/internal/transport-revision";

const CAPABILITIES = Object.freeze([
  "runtime.pi-local.v1",
  "model-download.v1",
  "approval.v1",
  "session-model-selection.v1",
  "tools.workspace-files.v1",
]);
const AUTO_SELECTION = ModelSelection.create({ selection: { $case: "auto", auto: true } });

export type {
  CreateModelAwareRuntimeOptions,
  ModelAwareRuntime,
} from "@lyonbrown/agentd/internal/model-aware-runtime-types";

export function createModelAwareRuntime(
  options: CreateModelAwareRuntimeOptions,
): ModelAwareRuntime {
  return new ModelAwareRuntimeHost(options);
}

// This state owner intentionally remains cohesive above the 300-line review trigger:
// activation, downloads, projections, rotation and resource cleanup already live in separate modules.
class ModelAwareRuntimeHost implements RuntimeHost, ModelAwareRuntime {
  readonly host: RuntimeHost = this;
  private modelStatus: LocalModelStatus;
  private activation: ActivationPhase = "inactive";
  private runtime: AgentRuntime | undefined;
  private readonly leaseOwnership = new ModelLeaseOwnership();
  private readonly runtimeCommands = new RuntimeCommandSerialQueue();
  private revision = 1n;
  private current: RuntimeSnapshot;
  private readonly feeds = new Set<RuntimeFeed>();
  private readonly unsubscribeModel: () => void;
  private readonly unsubscribeDecision: (() => void) | undefined;
  private runtimeFeed: ReturnType<AgentRuntime["snapshots"]> | undefined;
  private readonly activationController: ModelActivationController;
  private rotationTask: Promise<AgentRuntime> | undefined;
  private activeSessionId: string;
  private closeTask: Promise<void> | undefined;
  private closed = false;

  constructor(private readonly options: CreateModelAwareRuntimeOptions) {
    asSessionId(options.sessionId);
    this.activeSessionId = options.sessionId;
    this.modelStatus = options.modelService.status();
    this.current = this.composeSnapshot();
    this.activationController = new ModelActivationController({
      modelService: options.modelService,
      eventStore: options.eventStore,
      sessionId: options.sessionId,
      createRuntime: options.createRuntime,
      isClosed: () => this.closed,
      onActivating: () => {
        this.activation = "activating";
        this.publish();
      },
      onFailed: () => {
        this.activation = "failed";
        this.publish();
      },
      onReady: (runtime, lease) => {
        this.acceptActivatedRuntime(runtime, lease);
      },
    });
    this.unsubscribeModel = options.modelService.subscribe((status) => {
      this.onModelStatus(status);
    });
    this.unsubscribeDecision = options.decisionModel?.subscribe(() => {
      this.publish();
    });
    if (this.modelStatus.phase === "ready") this.activationController.begin();
  }

  async execute(command: RuntimeCommand): Promise<RuntimeResult> {
    if (this.closed) return failedRuntimeResult({ code: "RUNTIME_STOPPED" });
    if (command.kind === "start-model-download" || command.kind === "cancel-model-download") {
      if (command.purpose === "decision") {
        const control = this.options.decisionModel;
        if (control === undefined)
          return failedRuntimeResult({ code: "MODEL_DOWNLOAD_UNAVAILABLE" });
        return command.kind === "start-model-download"
          ? control.start(command.modelId)
          : control.cancel(command.operationId);
      }
      return executeModelDownloadCommand({
        command,
        status: this.modelStatus,
        service: this.options.modelService,
        activationFailed: this.activation === "failed",
        retryActivation: () => {
          this.activationController.retry();
        },
      });
    }
    return this.runtimeCommands.run(() => this.executeRuntimeCommand(command));
  }

  private async executeRuntimeCommand(command: RuntimeCommand): Promise<RuntimeResult> {
    if (this.closed) return failedRuntimeResult({ code: "RUNTIME_STOPPED" });
    let runtime = this.runtime;
    if (runtime === undefined) {
      return failedRuntimeResult({ code: "MODEL_SETUP_REQUIRED" });
    }
    if (command.kind === "select-session-model" && command.selection.kind === "pinned") {
      if (
        command.selection.provider !== "local" ||
        command.selection.model !== this.modelStatus.model.id
      ) {
        return failedRuntimeResult({
          code: "INVALID_MODEL_ID",
          requestedModelId: `${command.selection.provider}/${command.selection.model}`,
          configuredModelId: `local/${this.modelStatus.model.id}`,
        });
      }
    }
    if (!runtimeCommandWithinBudget(runtime.snapshot(), command)) {
      return failedRuntimeResult({ code: "RESOURCE_EXHAUSTED" });
    }
    if (command.kind === "start-turn" && shouldRotateSession(runtime.snapshot(), command.text)) {
      if (!(await this.rotateSession(runtime))) {
        return failedRuntimeResult({ code: "RUNTIME_STOPPED" });
      }
      runtime = this.runtime;
      if (runtime === undefined) return failedRuntimeResult({ code: "RUNTIME_STOPPED" });
    }
    return createRuntimeHostAdapter(runtime).execute(command);
  }

  private async rotateSession(current: AgentRuntime): Promise<boolean> {
    const lease = this.leaseOwnership.active();
    if (lease === undefined || this.closed) return false;
    const pending = this.rotationTask;
    if (pending !== undefined) {
      await pending.catch(() => undefined);
      return this.runtime !== undefined && !this.closed;
    }
    this.runtimeFeed?.close();
    this.runtimeFeed = undefined;
    this.runtime = undefined;
    const task = rotateRuntimeSession({
      current,
      lease,
      modelId: this.modelStatus.model.id,
      eventStore: this.options.eventStore,
      createRuntime: this.options.createRuntime,
    });
    this.rotationTask = task;
    try {
      const next = await task;
      if (this.isClosed()) {
        await next.dispose();
        return false;
      }
      this.runtime = next;
      this.activeSessionId = next.snapshot().sessionId;
      this.publish();
      this.observeRuntime(next);
      return true;
    } catch {
      await this.leaseOwnership.retire(lease);
      if (!this.isClosed()) {
        this.activation = "failed";
        this.publish();
      }
      return false;
    } finally {
      if (this.rotationTask === task) this.rotationTask = undefined;
    }
  }

  private isClosed(): boolean {
    return this.closed;
  }

  snapshot(): RuntimeSnapshot {
    return RuntimeSnapshot.create(this.current);
  }

  async *subscribe(afterRevision: bigint, signal: AbortSignal): AsyncIterable<RuntimeEvent> {
    let lastRevision = assertProtocolRevision(afterRevision);
    const feed = latestRuntimeFeed();
    if (signal.aborted || this.closed) return;
    this.feeds.add(feed);
    const abort = (): void => {
      feed.close();
    };
    signal.addEventListener("abort", abort, { once: true });
    if (this.current.revision > lastRevision) feed.publish(this.asEvent());
    try {
      for (;;) {
        const next = await feed.next();
        if (next.done) return;
        const revision = assertProtocolRevision(next.value.revision);
        if (revision <= lastRevision) continue;
        yield next.value;
        lastRevision = revision;
      }
    } finally {
      signal.removeEventListener("abort", abort);
      this.feeds.delete(feed);
      feed.close();
    }
  }

  capabilities(): readonly string[] {
    return this.options.decisionModel === undefined
      ? CAPABILITIES
      : [...CAPABILITIES, "decision-model-install.v1"];
  }

  close(): Promise<void> {
    this.closeTask ??= this.closeOwned();
    return this.closeTask;
  }

  private onModelStatus(status: LocalModelStatus): void {
    if (this.closed) return;
    this.modelStatus = status;
    this.publish();
    if (status.phase === "ready" && this.activation !== "failed") {
      this.activationController.begin();
    }
  }

  private acceptActivatedRuntime(runtime: AgentRuntime, lease: LocalModelLease): void {
    const sessionId = runtime.snapshot().sessionId;
    const feed = runtime.snapshots();
    this.leaseOwnership.accept(lease);
    this.runtime = runtime;
    this.activeSessionId = sessionId;
    this.activation = "inactive";
    this.publish();
    this.observeRuntime(runtime, feed);
  }

  private observeRuntime(runtime: AgentRuntime, feed = runtime.snapshots()): void {
    this.runtimeFeed = feed;
    void (async () => {
      try {
        for (;;) {
          const next = await feed.next();
          if (next.done) return;
          if (this.closed || this.runtime !== runtime) return;
          this.publish();
        }
      } catch {
        if (!this.closed) {
          this.activation = "failed";
          this.publish();
        }
      }
    })();
  }

  private publish(): void {
    if (this.closed) return;
    this.revision = nextTransportRevision(this.revision);
    this.current = this.composeSnapshot();
    const event = this.asEvent();
    for (const feed of this.feeds) feed.publish(event);
  }

  private composeSnapshot(): RuntimeSnapshot {
    const runtimeSnapshot =
      this.runtime === undefined ? undefined : projectRuntimeSnapshot(this.runtime.snapshot());
    const modelSelection = runtimeSnapshot?.modelSelection ?? AUTO_SELECTION;
    return RuntimeSnapshot.create({
      ...(runtimeSnapshot ?? {}),
      protocolVersion: PROTOCOL_VERSION,
      revision: this.revision,
      sessionId: runtimeSnapshot?.sessionId ?? this.activeSessionId,
      phase: runtimeSnapshot?.phase ?? projectSetupRuntimePhase(this.modelStatus, this.activation),
      modelSelection,
      modelSetup: projectModelSetup(this.modelStatus, this.activation),
      decisionModel: projectDecisionModel(this.options.decisionModel?.status()),
      modelCatalog: modelCatalog(this.modelStatus),
      modelPolicy: ModelPolicy.create({ active: modelSelection }),
      route: RouteSummary.create({
        selected: { provider: "local", model: this.modelStatus.model.id },
        reason: modelSelection.selection?.$case === "pinned" ? "session pinned" : "local default",
        fallbacks: [],
      }),
    });
  }

  private asEvent(): RuntimeEvent {
    return RuntimeEvent.create({
      protocolVersion: PROTOCOL_VERSION,
      revision: this.current.revision,
      snapshot: this.current,
    });
  }

  private async closeOwned(): Promise<void> {
    this.closed = true;
    this.unsubscribeDecision?.();
    await closeModelAwareResources({
      unsubscribeModel: this.unsubscribeModel,
      runtimeFeed: this.runtimeFeed,
      clientFeeds: this.feeds,
      runtime: this.runtime,
      leases: this.leaseOwnership.owned(),
      modelService: this.options.modelService,
      activation: this.activationController,
      eventStore: this.options.eventStore,
    });
    this.feeds.clear();
  }
}

function modelCatalog(status: LocalModelStatus): ModelCatalog {
  return ModelCatalog.create({
    models: [
      ModelOption.create({
        route: { provider: "local", model: status.model.id },
        tier: ModelTier.MODEL_TIER_WORKER,
        locality: ModelLocality.MODEL_LOCALITY_LOCAL,
        contextWindow: 4_096,
        maxOutputTokens: 2_048,
        supportsTools: true,
        supportsStreaming: true,
        supportsJson: false,
        supportsThinking: false,
        selectableForWorker: true,
      }),
    ],
  });
}
