import { randomUUID } from "node:crypto";

import { parseAgentRuntimeEvent } from "@lyonbrown/agent-domain";

import { runtimeError } from "@lyonbrown/agent-runtime/internal/errors";
import { RuntimeEventWriter } from "@lyonbrown/agent-runtime/internal/event-writer";
import { recoverRuntimeState } from "@lyonbrown/agent-runtime/internal/recovery";
import { DefaultAgentRuntime } from "@lyonbrown/agent-runtime/internal/runtime";
import type { LatestSnapshotFeed } from "@lyonbrown/agent-runtime/internal/snapshot-feed";
import type {
  AgentRuntime,
  CreateAgentRuntimeOptions,
} from "@lyonbrown/agent-runtime/internal/types";

export async function createAgentRuntime(
  options: CreateAgentRuntimeOptions,
): Promise<AgentRuntime> {
  const state = await recoverRuntimeState(options.sessionId, options.eventStore);
  const feeds = new Set<LatestSnapshotFeed>();
  const writer = new RuntimeEventWriter({
    sessionId: options.sessionId,
    eventStore: options.eventStore,
    initialState: state,
    createEventId: options.createEventId ?? randomUUID,
    now: options.now ?? (() => new Date().toISOString()),
    publish: (snapshot) => {
      for (const feed of feeds) feed.publish(snapshot);
    },
  });
  if (state.phase === "uninitialized") {
    await writer.append(
      parseAgentRuntimeEvent({ type: "session_opened", sessionId: options.sessionId }),
    );
  }
  try {
    const runtime = new DefaultAgentRuntime(options, writer, feeds);
    runtime.resumeQueuedFollowUps();
    return runtime;
  } catch {
    throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
  }
}
