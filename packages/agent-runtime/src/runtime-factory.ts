import { randomUUID } from "node:crypto";

import { parseAgentRuntimeEvent } from "@lyonbrown/agent-domain";

import { runtimeError } from "./errors.ts";
import { RuntimeEventWriter } from "./event-writer.ts";
import { recoverRuntimeState } from "./recovery.ts";
import { DefaultAgentRuntime } from "./runtime.ts";
import type { LatestSnapshotFeed } from "./snapshot-feed.ts";
import type { AgentRuntime, CreateAgentRuntimeOptions } from "./types.ts";

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
