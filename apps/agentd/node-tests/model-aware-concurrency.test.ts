import type { AgentRuntimeState } from "@lyonbrown/agent-domain";
import type { LocalModelLease, LocalModelService, LocalModelStatus } from "@lyonbrown/local-model";
import { DEFAULT_LOCAL_MODEL } from "@lyonbrown/local-model";
import { openEventStore } from "@lyonbrown/event-store";
import { ModelSetupPhase } from "@lyonbrown/protocol";
import { describe, expect, it, vi } from "vitest";

import { createModelAwareRuntime } from "../node-src/model-aware-runtime.ts";
import { MAX_SESSION_STEERING_PER_TURN, MAX_SESSION_TURNS } from "../node-src/runtime-limits.ts";
import { FakeAgentRuntime, stateFrom } from "./runtime-adapter-support.ts";

const SESSION_ID = "e".repeat(64);

function lease(): LocalModelLease {
  return {
    stream: async function* () {
      yield { type: "done", reason: "stop" } as const;
    },
    dispose: vi.fn(async () => undefined),
  };
}

class ReadyModelService implements LocalModelService {
  readonly startDownload = vi.fn(() => ({ kind: "already-ready" }) as const);
  readonly cancelDownload = vi.fn(() => ({ kind: "no-active-operation" }) as const);
  readonly dispose = vi.fn(async () => undefined);

  constructor(private readonly modelLease: LocalModelLease) {}

  status(): LocalModelStatus {
    return {
      model: DEFAULT_LOCAL_MODEL,
      phase: "ready",
      operationId: null,
      downloadedBytes: 0,
      totalBytes: null,
      failure: null,
      selectionReason: "hardware-compatible",
      allowedActions: [],
    };
  }

  subscribe(): () => void {
    return () => undefined;
  }

  async load() {
    return { kind: "ready", model: DEFAULT_LOCAL_MODEL, lease: this.modelLease } as const;
  }
}

function completedState(count: number): AgentRuntimeState {
  const base = stateFrom([{ type: "session_opened" }], SESSION_ID);
  return {
    ...base,
    phase: "completed",
    turns: Array.from({ length: count }, (_, index) => ({
      userText: `turn-${String(index)}`,
      assistantText: "",
      status: "completed" as const,
      turn: index + 1,
      steering: [],
      activities: [],
    })),
  };
}

async function waitReady(runtime: ReturnType<typeof createModelAwareRuntime>): Promise<void> {
  await vi.waitFor(async () => {
    expect((await runtime.host.snapshot()).modelSetup?.phase).toBe(
      ModelSetupPhase.MODEL_SETUP_PHASE_READY,
    );
  });
}

class UpdatingRuntime extends FakeAgentRuntime {
  override async followUp(input: string): Promise<void> {
    await Promise.resolve();
    await super.followUp(input);
    this.state = { ...this.state, queuedFollowUps: [...this.state.queuedFollowUps, input] };
  }

  override async steer(input: string): Promise<void> {
    await Promise.resolve();
    await super.steer(input);
    const turns = [...this.state.turns];
    const current = turns.at(-1);
    if (current !== undefined) {
      turns[turns.length - 1] = { ...current, steering: [...current.steering, input] };
      this.state = { ...this.state, turns };
    }
  }
}

describe("model-aware runtime command serialization", () => {
  it("serializes concurrent follow-ups so only the exact final turn fits", async () => {
    const agent = new UpdatingRuntime(completedState(MAX_SESSION_TURNS - 1));
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: new ReadyModelService(lease()),
      eventStore: openEventStore({ testOnlyMemory: true }),
      createRuntime: vi.fn(async () => agent),
    });
    await waitReady(runtime);

    await expect(
      Promise.all([
        runtime.host.execute({ kind: "queue-follow-up", text: "fits" }),
        runtime.host.execute({ kind: "queue-follow-up", text: "overflow" }),
      ]),
    ).resolves.toEqual([{ ok: true }, { ok: false, error: { code: "RESOURCE_EXHAUSTED" } }]);
    expect(agent.followUps).toEqual(["fits"]);
    await runtime.close();
  });

  it("serializes concurrent steering so only the sixteenth instruction fits", async () => {
    const state = completedState(1);
    const current = state.turns.at(0);
    if (current === undefined) throw new Error("active turn fixture missing");
    const agent = new UpdatingRuntime({
      ...state,
      phase: "running",
      turns: [
        {
          ...current,
          status: "running",
          steering: Array.from({ length: MAX_SESSION_STEERING_PER_TURN - 1 }, () => "steer"),
        },
      ],
    });
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: new ReadyModelService(lease()),
      eventStore: openEventStore({ testOnlyMemory: true }),
      createRuntime: vi.fn(async () => agent),
    });
    await waitReady(runtime);

    await expect(
      Promise.all([
        runtime.host.execute({ kind: "steer-current-turn", text: "fits" }),
        runtime.host.execute({ kind: "steer-current-turn", text: "overflow" }),
      ]),
    ).resolves.toEqual([{ ok: true }, { ok: false, error: { code: "RESOURCE_EXHAUSTED" } }]);
    expect(agent.steering).toEqual(["fits"]);
    await runtime.close();
  });

  it("does not make close wait for a hung serialized command", async () => {
    const agent = new UpdatingRuntime(completedState(0));
    let release = (): void => undefined;
    const followUp = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    agent.followUp = followUp;
    const runtime = createModelAwareRuntime({
      sessionId: SESSION_ID,
      modelService: new ReadyModelService(lease()),
      eventStore: openEventStore({ testOnlyMemory: true }),
      createRuntime: vi.fn(async () => agent),
    });
    await waitReady(runtime);

    const pending = runtime.host.execute({ kind: "queue-follow-up", text: "hung" });
    await vi.waitFor(() => expect(followUp).toHaveBeenCalledOnce());
    await expect(runtime.close()).resolves.toBeUndefined();
    release();
    await expect(pending).resolves.toEqual({ ok: true });
  });
});
