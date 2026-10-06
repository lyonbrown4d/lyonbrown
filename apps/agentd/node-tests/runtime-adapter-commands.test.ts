import { AgentRuntimeError } from "@lyonbrown/agent-runtime";
import { describe, expect, test } from "vitest";

import { createRuntimeHostAdapter } from "@lyonbrown/agentd/internal/runtime-adapter";
import {
  activeState,
  FakeAgentRuntime,
  openedState,
  stateFrom,
} from "@lyonbrown/agentd/test/runtime-adapter-support";

describe("runtime host adapter commands", () => {
  test("maps start, steer, follow-up, and cancel to the agent runtime", async () => {
    const runtime = new FakeAgentRuntime(activeState());
    const host = createRuntimeHostAdapter(runtime);

    await expect(host.execute({ kind: "steer-current-turn", text: "focus" })).resolves.toEqual({
      ok: true,
    });
    await expect(host.execute({ kind: "queue-follow-up", text: "summarize" })).resolves.toEqual({
      ok: true,
    });
    await expect(host.execute({ kind: "cancel-turn" })).resolves.toEqual({ ok: true });
    runtime.state = openedState();
    await expect(host.execute({ kind: "start-turn", text: "hello" })).resolves.toEqual({
      ok: true,
    });

    expect(runtime.steering).toEqual(["focus"]);
    expect(runtime.followUps).toEqual(["summarize"]);
    expect(runtime.cancelCalls).toBe(1);
    expect(runtime.starts).toEqual(["hello"]);
  });

  test("forwards a correlated approval decision to the runtime", async () => {
    const runtime = new FakeAgentRuntime(
      stateFrom([
        { type: "session_opened" },
        { type: "user_message_submitted", text: "write" },
        { type: "run_started" },
        { type: "turn_started", turn: 1 },
        {
          type: "approval_requested",
          requestId: "call-1",
          toolName: "write",
          risk: "medium",
          permissions: ["workspace.write"],
          argumentsPreview: { path: "notes.txt" },
        },
      ]),
    );
    const host = createRuntimeHostAdapter(runtime);

    await expect(
      host.execute({
        kind: "respond-approval",
        requestId: "call-1",
        decision: "allow-once",
      }),
    ).resolves.toEqual({ ok: true });
    expect(runtime.approvalResponses).toEqual([{ requestId: "call-1", decision: "allow-once" }]);
  });

  test("persists session model selection through the runtime", async () => {
    const runtime = new FakeAgentRuntime(openedState());
    const host = createRuntimeHostAdapter(runtime);

    await expect(
      host.execute({
        kind: "select-session-model",
        selection: { kind: "pinned", provider: "local", model: "qwen-local" },
      }),
    ).resolves.toEqual({ ok: true });
    expect(runtime.modelSelections).toEqual([
      { kind: "pinned", provider: "local", model: "qwen-local" },
    ]);
  });

  test("rejects empty text and commands that conflict with projected state", async () => {
    const runtime = new FakeAgentRuntime(openedState());
    const host = createRuntimeHostAdapter(runtime);

    await expect(host.execute({ kind: "start-turn", text: " \n " })).resolves.toEqual({
      ok: false,
      error: { code: "EMPTY_TEXT" },
    });
    runtime.controlError = new AgentRuntimeError("NO_ACTIVE_TURN", "no active turn");
    await expect(host.execute({ kind: "steer-current-turn", text: "focus" })).resolves.toEqual({
      ok: false,
      error: { code: "NO_ACTIVE_TURN" },
    });
    runtime.controlError = undefined;
    await expect(host.execute({ kind: "queue-follow-up", text: "later" })).resolves.toEqual({
      ok: true,
    });
    runtime.controlError = new AgentRuntimeError("NO_ACTIVE_TURN", "no active turn");
    await expect(host.execute({ kind: "cancel-turn" })).resolves.toEqual({
      ok: false,
      error: { code: "NO_ACTIVE_TURN" },
    });

    runtime.state = activeState();
    runtime.controlError = undefined;
    runtime.startError = new AgentRuntimeError("SESSION_BUSY", "already active");
    await expect(host.execute({ kind: "start-turn", text: "second" })).resolves.toEqual({
      ok: false,
      error: { code: "TURN_ALREADY_ACTIVE" },
    });
    expect(runtime.starts).toEqual([]);
    expect(runtime.steering).toEqual([]);
    expect(runtime.followUps).toEqual(["later"]);
    expect(runtime.cancelCalls).toBe(0);
  });

  test("lets the runtime distinguish a live turn from recovered unresolved work", async () => {
    const runtime = new FakeAgentRuntime(activeState());
    runtime.startError = new AgentRuntimeError("TERMINAL_SESSION", "unresolved recovered work");
    const host = createRuntimeHostAdapter(runtime);

    await expect(host.execute({ kind: "start-turn", text: "retry" })).resolves.toEqual({
      ok: false,
      error: { code: "RUNTIME_STOPPED" },
    });
    expect(runtime.starts).toEqual([]);
  });

  test("maps runtime failures without exposing error text", async () => {
    const runtime = new FakeAgentRuntime(openedState());
    const host = createRuntimeHostAdapter(runtime);
    runtime.startError = new AgentRuntimeError("SESSION_BUSY", "secret busy detail");

    await expect(host.execute({ kind: "start-turn", text: "hello" })).resolves.toEqual({
      ok: false,
      error: { code: "TURN_ALREADY_ACTIVE" },
    });

    runtime.startError = new AgentRuntimeError("KERNEL_FAILURE", "secret kernel detail");
    await expect(host.execute({ kind: "start-turn", text: "hello" })).resolves.toEqual({
      ok: false,
      error: { code: "RUNTIME_STOPPED" },
    });

    runtime.startError = new Error("raw secret");
    await expect(host.execute({ kind: "start-turn", text: "hello" })).resolves.toEqual({
      ok: false,
      error: { code: "RUNTIME_STOPPED" },
    });
  });

  test("maps recovered terminal failures decided by the runtime", async () => {
    const runtime = new FakeAgentRuntime(
      stateFrom([
        { type: "session_opened" },
        { type: "user_message_submitted", text: "question" },
        { type: "run_started" },
        { type: "turn_started", turn: 1 },
        {
          type: "tool_outcome_unknown",
          callId: "call-1",
          toolName: "shell",
          code: "TOOL_TIMEOUT",
          message: "outcome unknown",
        },
        { type: "run_failed", code: "TOOL_TIMEOUT", message: "outcome unknown" },
      ]),
    );
    runtime.startError = new AgentRuntimeError("TERMINAL_SESSION", "terminal session");
    runtime.controlError = new AgentRuntimeError("TERMINAL_SESSION", "terminal session");
    const host = createRuntimeHostAdapter(runtime);

    await expect(host.execute({ kind: "start-turn", text: "retry" })).resolves.toEqual({
      ok: false,
      error: { code: "RUNTIME_STOPPED" },
    });
    await expect(host.execute({ kind: "cancel-turn" })).resolves.toEqual({
      ok: false,
      error: { code: "RUNTIME_STOPPED" },
    });
    expect(runtime.starts).toEqual([]);
    expect(runtime.cancelCalls).toBe(0);
  });

  test("advertises no capabilities that the domain runtime does not provide", () => {
    const host = createRuntimeHostAdapter(new FakeAgentRuntime(openedState()));
    expect(host.capabilities()).toEqual([]);
    expect(Object.isFrozen(host.capabilities())).toBe(true);
  });
});
