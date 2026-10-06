import type { AgentRuntime, RuntimeSnapshotFeed } from "@lyonbrown/agent-runtime";
import type { EventStore } from "@lyonbrown/event-store";
import type { LocalModelLease, LocalModelService } from "@lyonbrown/local-model";

import { collectCleanupFailure } from "@lyonbrown/agentd/internal/cleanup";
import type { RuntimeFeed } from "@lyonbrown/agentd/internal/latest-runtime-feed";
import type { ModelActivationController } from "@lyonbrown/agentd/internal/model-activation";

export async function closeModelAwareResources(options: {
  readonly unsubscribeModel: () => void;
  readonly runtimeFeed: RuntimeSnapshotFeed | undefined;
  readonly clientFeeds: ReadonlySet<RuntimeFeed>;
  readonly runtime: AgentRuntime | undefined;
  readonly leases: readonly LocalModelLease[];
  readonly modelService: LocalModelService;
  readonly activation: ModelActivationController;
  readonly eventStore: EventStore;
}): Promise<void> {
  const failures: unknown[] = [];
  await collectCleanupFailure(() => {
    options.unsubscribeModel();
  }, failures);
  options.runtimeFeed?.close();
  for (const feed of options.clientFeeds) feed.close();
  await collectCleanupFailure(() => options.runtime?.dispose(), failures);
  for (const lease of options.leases) {
    await collectCleanupFailure(() => lease.dispose(), failures);
  }
  await collectCleanupFailure(() => options.modelService.dispose(), failures);
  await options.activation.drain(failures);
  await collectCleanupFailure(() => {
    options.eventStore.close();
  }, failures);
  if (failures.length > 0) throw new AggregateError(failures, "runtime resource cleanup failed");
}
