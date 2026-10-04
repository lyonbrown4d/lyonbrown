import { randomUUID } from "node:crypto";

import { parseAgentRuntimeEvent } from "@lyonbrown/agent-domain";

import { runtimeError } from "./errors.js";
import { RuntimeEventWriter } from "./event-writer.js";
import { recoverRuntimeState } from "./recovery.js";
import { DefaultAgentRuntime } from "./runtime.js";
import type { LatestSnapshotFeed } from "./snapshot-feed.js";
import type { AgentRuntime, CreateAgentRuntimeOptions } from "./types.js";

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
