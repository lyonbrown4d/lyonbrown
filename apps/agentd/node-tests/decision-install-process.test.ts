import { fork, type ChildProcess } from "node:child_process";
import { EventEmitter } from "node:events";

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  parseDecisionInstallProgress,
  runDecisionInstallProcess,
} from "@lyonbrown/agentd/internal/decision-install-process";
import { DECISION_MODEL_TOTAL_BYTES } from "@lyonbrown/agentd/internal/decision-model-state";

vi.mock("node:child_process", () => ({ fork: vi.fn() }));

class Child extends EventEmitter {
  connected = true;
  send = vi.fn();
  kill = vi.fn(() => {
    this.emit("exit", null);
    return true;
  });
}

function fixture() {
  const child = new Child();
  vi.mocked(fork).mockReturnValue(child as unknown as ChildProcess);
  const abort = new AbortController();
  const progress = vi.fn();
  const task = runDecisionInstallProcess("validated-root", "operation", abort.signal, progress);
  return { child, abort, progress, task };
}

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe("decision installation process ownership", () => {
  it("spawns only the fixed built worker and waits for both completion and clean exit", async () => {
    const f = fixture();
    expect(fork).toHaveBeenCalledWith(
      expect.any(URL),
      ["validated-root", "operation"],
      expect.objectContaining({ execArgv: [], windowsHide: true }),
    );
    f.child.emit("message", { type: "progress", phase: "verifying" });
    expect(f.progress).toHaveBeenCalledWith({ phase: "verifying" });
    f.child.emit("message", { type: "completed" });
    f.child.emit("exit", 0);
    await expect(f.task).resolves.toBeUndefined();
    f.abort.abort();
    expect(f.child.send).not.toHaveBeenCalled();
  });

  it("fails closed when a child exits without a completion marker", async () => {
    const f = fixture();
    f.child.emit("exit", 0);
    await expect(f.task).rejects.toThrow("decision_model_install_failed");
  });

  it("cancels cooperatively and kills an unresponsive owned worker after the cleanup budget", async () => {
    vi.useFakeTimers();
    const f = fixture();
    const result = expect(f.task).rejects.toThrow("decision_model_install_cancelled");
    f.abort.abort();
    expect(f.child.send).toHaveBeenCalledWith({ type: "cancel" }, expect.any(Function));
    await vi.advanceTimersByTimeAsync(15_000);
    await result;
    expect(f.child.kill).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("rejects malformed or regressing progress without exposing it", async () => {
    const f = fixture();
    f.child.emit("message", {
      type: "progress",
      phase: "downloading",
      downloadedBytes: 20,
      totalBytes: DECISION_MODEL_TOTAL_BYTES,
    });
    f.child.emit("message", {
      type: "progress",
      phase: "downloading",
      downloadedBytes: 10,
      totalBytes: DECISION_MODEL_TOTAL_BYTES,
    });
    f.child.emit("message", { type: "completed" });
    f.child.emit("exit", 0);
    await expect(f.task).rejects.toThrow("decision_model_install_failed");
    expect(f.progress).toHaveBeenCalledOnce();
  });

  it("handles spawn failure with a sanitized error", async () => {
    const f = fixture();
    f.child.emit("error", new Error("credentials=secret"));
    await expect(f.task).rejects.toThrow("decision_model_install_process_failed");
  });

  it("does not spawn for an already-aborted request", async () => {
    await expect(
      runDecisionInstallProcess("root", "op", AbortSignal.abort(), vi.fn()),
    ).rejects.toThrow("cancelled");
    expect(fork).not.toHaveBeenCalled();
  });

  it.each([
    null,
    "text",
    {},
    { type: "progress", phase: "unexpected" },
    {
      type: "progress",
      phase: "downloading",
      downloadedBytes: -1,
      totalBytes: DECISION_MODEL_TOTAL_BYTES,
    },
    { type: "progress", phase: "downloading", downloadedBytes: 0, totalBytes: 0 },
    { type: "progress", phase: "downloading", downloadedBytes: Infinity },
  ])("rejects malformed IPC %#", (value) => {
    expect(parseDecisionInstallProgress(value)).toBeNull();
  });
});
