import { EventEmitter } from "node:events";
import { RuntimeEvent } from "@lyonbrown/protocol";
import { describe, expect, it, vi } from "vitest";
import { pumpSnapshotStream, type SnapshotWriteCall } from "../node-src/stream.js";

class ControlledCall extends EventEmitter implements SnapshotWriteCall {
  public cancelled = false;
  public readonly callbacks: (() => void)[] = [];
  public readonly write = vi.fn((_event: RuntimeEvent, callback: () => void): boolean => {
    this.callbacks.push(callback);
    return false;
  });
  public readonly end = vi.fn();
}

describe("pumpSnapshotStream", () => {
  it("waits for each write callback before pulling the next event", async () => {
    const call = new ControlledCall();
    let pulls = 0;
    async function* events(): AsyncIterable<RuntimeEvent> {
      pulls += 1;
      yield RuntimeEvent.create({ revision: 1n });
      pulls += 1;
      yield RuntimeEvent.create({ revision: 2n });
    }

    const pumping = pumpSnapshotStream(call, events());
    await vi.waitFor(() => {
      expect(call.write).toHaveBeenCalledTimes(1);
    });
    expect(pulls).toBe(1);
    call.callbacks.shift()?.();
    await vi.waitFor(() => {
      expect(call.write).toHaveBeenCalledTimes(2);
    });
    expect(pulls).toBe(2);
    call.callbacks.shift()?.();
    await pumping;
    expect(call.end).toHaveBeenCalledOnce();
  });

  it("aborts the producer and does not end an already-cancelled call", async () => {
    const call = new ControlledCall();
    let observedSignal: AbortSignal | undefined;
    async function* events(signal: AbortSignal): AsyncIterable<RuntimeEvent> {
      observedSignal = signal;
      yield RuntimeEvent.create({ revision: 1n });
      await new Promise<void>((resolve) => {
        signal.addEventListener("abort", () => {
          resolve();
        });
      });
    }

    const pumping = pumpSnapshotStream(call, (signal) => events(signal));
    await vi.waitFor(() => {
      expect(call.write).toHaveBeenCalledOnce();
    });
    call.cancelled = true;
    call.emit("cancelled");
    await pumping;
    expect(observedSignal?.aborted).toBe(true);
    expect(call.end).not.toHaveBeenCalled();
  });
});
