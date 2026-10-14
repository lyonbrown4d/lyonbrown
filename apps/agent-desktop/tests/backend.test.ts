import { expect, test, vi } from "vitest";
import type { AgentClient } from "@lyonbrown/agent-client";
import { DesktopBackend } from "@lyonbrown/agent-desktop/internal/main/backend";
import { snapshot } from "@lyonbrown/agent-desktop/test/support";
function fixture() {
  const enqueue = vi.fn(async () => ({ commandId: "command", accepted: true as const }));
  const close = vi.fn(async () => {});
  const client: AgentClient = {
    health: async () => ({ protocolVersion: 1, status: "serving" }),
    handshake: async () => ({
      protocolVersion: 1,
      serverVersion: "0",
      serverInstanceId: "epoch",
      scopeId: "scope",
      capabilities: [],
    }),
    enqueue,
    close,
    getSnapshot: async () => snapshot(),
    replaceBinding: () => {},
    subscribeEvents: async function* ({ signal }) {
      await new Promise<void>((resolve) =>
        signal?.addEventListener("abort", () => resolve(), { once: true }),
      );
      yield { protocolVersion: 1, revision: 2n, snapshot: snapshot({ revision: 2n }) };
    },
  };
  const backend = new DesktopBackend(
    {
      connect: async () => ({
        client,
        snapshot: snapshot(),
        serverInstanceId: "epoch",
        scopeId: "scope",
        source: "reused",
      }),
    },
    () => {},
  );
  return { backend, enqueue, close, client };
}
test("connects a launcher, sends through its authenticated client, and closes only that client", async () => {
  const { backend, enqueue, close } = fixture();
  await backend.connect();
  expect(backend.state().status).toBe("connected");
  expect(
    await backend.act({
      type: "send",
      text: "hello",
      token: backend.state().actionToken ?? "fixture:1",
    }),
  ).toEqual({ ok: true, error: null });
  expect(enqueue).toHaveBeenCalledWith({ type: "start-turn", text: "hello" });
  await backend.close();
  expect(close).toHaveBeenCalledOnce();
  expect(backend.state().status).toBe("disconnected");
});
test("blocks stale and disconnected mutations before reaching gRPC", async () => {
  const { backend, enqueue } = fixture();
  expect(
    (
      await backend.act({
        type: "send",
        text: "x",
        token: backend.state().actionToken ?? "fixture:1",
      })
    ).ok,
  ).toBe(false);
  await backend.connect();
  expect((await backend.act({ type: "send", text: "x", token: "fixture:0" })).error).toBe(
    "STALE_STATE",
  );
  expect(
    (await backend.act({ type: "approve", token: backend.state().actionToken ?? "fixture:1" })).ok,
  ).toBe(false);
  expect(
    (await backend.act({ type: "cancel", token: backend.state().actionToken ?? "fixture:1" })).ok,
  ).toBe(false);
  expect(enqueue).not.toHaveBeenCalled();
  await backend.close();
});
test("sanitizes startup errors and supports explicit retry", async () => {
  const launcher = { connect: vi.fn().mockRejectedValueOnce(new Error("secret-token")) };
  const backend = new DesktopBackend(launcher, () => {});
  expect((await backend.connect()).error).toBe("BACKEND_UNAVAILABLE");
  expect(JSON.stringify(backend.state())).not.toContain("secret-token");
  await backend.close();
});
test("rotates the mutation token even when reconnect returns the same snapshot revision", async () => {
  const { backend, enqueue } = fixture();
  await backend.connect();
  const first = backend.state().actionToken;
  await backend.connect();
  const second = backend.state().actionToken;
  expect(second).not.toBe(first);
  expect((await backend.act({ type: "cancel", token: first })).ok).toBe(false);
  expect(enqueue).not.toHaveBeenCalled();
  await backend.close();
});
