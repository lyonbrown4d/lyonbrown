import type { RuntimeEvent } from "@lyonbrown/protocol";

export interface SnapshotWriteCall {
  readonly cancelled: boolean;
  write(event: RuntimeEvent, callback: () => void): boolean;
  once(event: "cancelled", listener: () => void): this;
  removeListener(event: "cancelled", listener: () => void): this;
  end(): void;
}

type EventSource =
  | AsyncIterable<RuntimeEvent>
  | ((signal: AbortSignal) => AsyncIterable<RuntimeEvent>);

function isCancelled(call: SnapshotWriteCall, signal: AbortSignal): boolean {
  return call.cancelled || signal.aborted;
}

function writeOne(
  call: SnapshotWriteCall,
  event: RuntimeEvent,
  signal: AbortSignal,
): Promise<void> {
  return new Promise((resolve) => {
    let settled = false;
    const settle = () => {
      if (settled) return;
      settled = true;
      signal.removeEventListener("abort", settle);
      resolve();
    };
    signal.addEventListener("abort", settle, { once: true });
    call.write(event, settle);
  });
}

export async function pumpSnapshotStream(
  call: SnapshotWriteCall,
  source: EventSource,
  shutdownSignal?: AbortSignal,
): Promise<void> {
  const controller = new AbortController();
  const cancel = () => {
    controller.abort();
  };
  call.once("cancelled", cancel);
  shutdownSignal?.addEventListener("abort", cancel, { once: true });
  if (call.cancelled || shutdownSignal?.aborted === true) cancel();
  try {
    const events = typeof source === "function" ? source(controller.signal) : source;
    for await (const event of events) {
      if (isCancelled(call, controller.signal)) break;
      await writeOne(call, event, controller.signal);
      if (isCancelled(call, controller.signal)) break;
    }
    if (!isCancelled(call, controller.signal)) call.end();
  } finally {
    controller.abort();
    call.removeListener("cancelled", cancel);
    shutdownSignal?.removeEventListener("abort", cancel);
  }
}
