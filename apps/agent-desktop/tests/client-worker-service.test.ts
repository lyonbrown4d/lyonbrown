import { expect, test, vi } from "vitest";
import { ClientWorkerService } from "@lyonbrown/agent-desktop/internal/main/client-worker-service";
import { disconnectedState } from "@lyonbrown/agent-desktop/internal/main/projection";
function fixture() {
  const backend = {
    connect: vi.fn(async () => ({ ok: true, error: null })),
    act: vi.fn(async () => ({ ok: true, error: null })),
    close: vi.fn(async () => {}),
  };
  const port = {
    connected: () => true,
    send: vi.fn((_value: unknown, done: (error: Error | null) => void) => {
      done(null);
    }),
    disconnect: vi.fn(),
  };
  const service = new ClientWorkerService(() => backend, port);
  return { backend, port, service };
}
test("forwards only validated requests and closes its client on invalid input", async () => {
  const { service, backend, port } = fixture();
  await service.receive({ type: "connect", id: "1" });
  expect(backend.connect).toHaveBeenCalledOnce();
  await service.receive({ type: "action", id: "2", action: { type: "send" } });
  expect(backend.act).toHaveBeenCalledWith({ type: "send" });
  expect(port.send).toHaveBeenCalledWith(
    { type: "reply", id: "2", result: { ok: true, error: null } },
    expect.any(Function),
  );
  await service.receive({ type: "eval", id: "3" });
  expect(port.disconnect).toHaveBeenCalledOnce();
  await service.close();
  expect(backend.close).toHaveBeenCalledOnce();
});
test("coalesces in-flight snapshot updates rather than buffering unbounded frames", () => {
  const { service, port } = fixture();
  let done: (error: Error | null) => void = () => {};
  port.send.mockImplementation((_value, callback) => {
    done = callback;
  });
  service.publish(disconnectedState());
  service.publish({ ...disconnectedState(), revision: "2" });
  service.publish({ ...disconnectedState(), revision: "3" });
  expect(port.send).toHaveBeenCalledTimes(1);
  done(null);
  expect(port.send).toHaveBeenCalledTimes(2);
  expect(port.send.mock.calls[1]?.[0]).toMatchObject({ state: { revision: "3" } });
});
test("disposes on a failed send and acknowledges bounded close", async () => {
  const { service, port, backend } = fixture();
  port.send.mockImplementation((_value, done) => {
    done(new Error("gone"));
  });
  service.publish(disconnectedState());
  await vi.waitFor(() => expect(port.disconnect).toHaveBeenCalledOnce());
  expect(backend.close).toHaveBeenCalledOnce();
  const another = fixture();
  await another.service.receive({ type: "close", id: "1" });
  await vi.waitFor(() => expect(another.port.disconnect).toHaveBeenCalledOnce());
});
