// @vitest-environment node
import { EventEmitter } from "node:events";
import type { ChildProcess } from "node:child_process";
import { afterEach, expect, test, vi } from "vitest";
import { DesktopProcessBackend } from "@lyonbrown/agent-desktop/internal/main/process-backend";
import { disconnectedState } from "@lyonbrown/agent-desktop/internal/main/projection";
class Worker extends EventEmitter {
  connected = true;
  exitCode: number | null = null;
  signalCode: NodeJS.Signals | null = null;
  killed = false;
  send = vi.fn((request: { type: string; id: string }, callback: (error: Error | null) => void) => {
    callback(null);
    this.emit("message", { type: "reply", id: request.id, result: { ok: true, error: null } });
    if (request.type === "close") this.exit();
    return true;
  });
  kill(): boolean {
    this.killed = true;
    this.exit();
    return true;
  }
  exit(): void {
    this.connected = false;
    this.exitCode = 0;
    this.emit("exit", 0);
  }
  process(): ChildProcess {
    return this as unknown as ChildProcess;
  }
}
afterEach(() => {
  vi.useRealTimers();
});
test("handles spawn failures and rejects malformed actions before IPC", async () => {
  const backend = new DesktopProcessBackend(
    () => {},
    () => {
      throw new Error("missing Node");
    },
  );
  expect(backend.state().error).toBe("BACKEND_UNAVAILABLE");
  expect((await backend.connect()).ok).toBe(false);
  await backend.close();
  expect((await backend.connect()).error).toBe("CLOSED");
  const worker = new Worker();
  const live = new DesktopProcessBackend(
    () => {},
    () => worker.process(),
  );
  expect((await live.act({ type: "force-stop" })).error).toBe("INVALID_ACTION");
  expect(worker.send).not.toHaveBeenCalled();
  await live.close();
  await live.close();
});
test("settles outstanding requests on worker process error", async () => {
  const worker = new Worker();
  worker.send.mockImplementation((_request, callback) => {
    callback(null);
    return true;
  });
  const backend = new DesktopProcessBackend(
    () => {},
    () => worker.process(),
  );
  const pending = backend.connect();
  worker.emit("error", new Error("worker exited"));
  expect((await pending).ok).toBe(false);
  await backend.close();
});
test("bounds graceful shutdown waiting for an unresponsive owned worker", async () => {
  vi.useFakeTimers();
  const worker = new Worker();
  worker.send.mockImplementation((request, callback) => {
    callback(null);
    worker.emit("message", { type: "reply", id: request.id, result: { ok: true, error: null } });
    return true;
  });
  const backend = new DesktopProcessBackend(
    () => {},
    () => worker.process(),
  );
  const closing = backend.close();
  await vi.advanceTimersByTimeAsync(2_000);
  await closing;
  expect(worker.killed).toBe(true);
});
test("recreates only its client worker after crash, ignoring retired child messages", async () => {
  const first = new Worker();
  const second = new Worker();
  const spawn = vi.fn().mockReturnValueOnce(first.process()).mockReturnValueOnce(second.process());
  const backend = new DesktopProcessBackend(() => {}, spawn);
  await backend.connect();
  first.exit();
  expect(backend.state().status).toBe("disconnected");
  expect((await backend.connect()).ok).toBe(true);
  expect(spawn).toHaveBeenCalledTimes(2);
  first.emit("message", { type: "state", state: { ...disconnectedState(), status: "connected" } });
  expect(backend.state().status).toBe("disconnected");
  second.emit("message", { type: "state", state: { ...disconnectedState(), status: "connected" } });
  expect(backend.state().status).toBe("connected");
  await backend.close();
  expect(second.exitCode).toBe(0);
});
test("retires a malformed worker and rejects late mutation requests", async () => {
  const worker = new Worker();
  const backend = new DesktopProcessBackend(
    () => {},
    () => worker.process(),
  );
  worker.emit("message", { type: "state", state: { token: "secret" } });
  expect(worker.killed).toBe(true);
  await backend.close();
  expect((await backend.act({})).ok).toBe(false);
});
test("ignores queued state after kill while process exit is still pending", async () => {
  const worker = new Worker();
  worker.kill = vi.fn(() => {
    worker.killed = true;
    return true;
  });
  const backend = new DesktopProcessBackend(
    () => {},
    () => worker.process(),
  );
  worker.emit("message", { type: "invalid" });
  worker.emit("message", {
    type: "state",
    state: { ...disconnectedState(), status: "connected", canSend: true, actionToken: "old:1" },
  });
  expect(backend.state().status).toBe("disconnected");
  expect(backend.state().canSend).toBe(false);
  worker.exit();
  await backend.close();
});
test("times out a stuck request and kills only its owned worker", async () => {
  vi.useFakeTimers();
  const worker = new Worker();
  worker.send.mockImplementation((_request, callback) => {
    callback(null);
    return true;
  });
  const backend = new DesktopProcessBackend(
    () => {},
    () => worker.process(),
  );
  const pending = backend.connect();
  await vi.advanceTimersByTimeAsync(35_000);
  expect((await pending).ok).toBe(false);
  expect(worker.killed).toBe(true);
  await backend.close();
});
