export async function collectCleanupFailure(
  operation: () => unknown,
  failures: unknown[],
): Promise<void> {
  try {
    await operation();
  } catch (error: unknown) {
    failures.push(error);
  }
}

export async function disposeActivationResources(
  runtime: AgentRuntime | undefined,
  lease: LocalModelLease,
): Promise<void> {
  const failures: unknown[] = [];
  if (runtime !== undefined) await collectCleanupFailure(() => runtime.dispose(), failures);
  await collectCleanupFailure(() => lease.dispose(), failures);
  if (failures.length > 0) {
    throw new AggregateError(failures, "runtime_activation_cleanup_failed");
  }
}

export async function drainActivationTask(task: Promise<void> | undefined): Promise<void> {
  if (task === undefined) return;
  let timer: NodeJS.Timeout | undefined;
  const timeout = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(() => {
      reject(new Error("runtime_activation_shutdown_timeout"));
    }, ACTIVATION_SHUTDOWN_TIMEOUT_MS);
    timer.unref();
  });
  try {
    await Promise.race([task, timeout]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}
import type { AgentRuntime } from "@lyonbrown/agent-runtime";
import type { LocalModelLease } from "@lyonbrown/local-model";

import { ACTIVATION_SHUTDOWN_TIMEOUT_MS } from "@lyonbrown/agentd/internal/runtime-limits";
