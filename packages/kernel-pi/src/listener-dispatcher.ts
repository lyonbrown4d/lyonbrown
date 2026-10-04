import { kernelError, type KernelRuntimeError } from "./errors.js";
import type { KernelEvent, KernelEventListener } from "./types.js";

export interface ListenerDispatcherOptions {
  readonly maxPendingEvents: number;
  readonly timeoutMs: number;
}

type DeliveryOutcome = "delivered" | "rejected" | "timed_out" | "cancelled";
type DiagnosticSink = (error: KernelRuntimeError) => void;

export class ListenerDispatcher {
  readonly #subscriptions = new Set<ListenerSubscription>();
  #disposed = false;

  constructor(
    private readonly options: ListenerDispatcherOptions,
    private readonly diagnostic: DiagnosticSink = () => undefined,
  ) {
    if (
      !Number.isSafeInteger(options.maxPendingEvents) ||
      options.maxPendingEvents <= 0 ||
      !Number.isSafeInteger(options.timeoutMs) ||
      options.timeoutMs <= 0
    ) {
      throw new TypeError("Listener delivery limits must be positive safe integers");
    }
  }

  subscribe(listener: KernelEventListener): () => void {
    if (this.#disposed) throw kernelError("SESSION_DISPOSED", "Kernel session is disposed");
    const subscription = new ListenerSubscription(listener, this.options, (error) => {
      this.#subscriptions.delete(subscription);
      try {
        this.diagnostic(error);
      } catch {
        // Internal diagnostics must never re-enter the durable execution path.
      }
    });
    this.#subscriptions.add(subscription);
    return () => {
      this.#subscriptions.delete(subscription);
      subscription.close();
    };
  }

  dispatch(event: KernelEvent): void {
    if (this.#disposed) return;
    for (const subscription of this.#subscriptions) subscription.enqueue(event);
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    for (const subscription of this.#subscriptions) subscription.close();
    this.#subscriptions.clear();
  }
}

class ListenerSubscription {
  readonly #queue: KernelEvent[] = [];
  #active = true;
  #draining = false;
  #current: AbortController | undefined;

  constructor(
    private readonly listener: KernelEventListener,
    private readonly options: ListenerDispatcherOptions,
    private readonly fail: DiagnosticSink,
  ) {}

  enqueue(event: KernelEvent): void {
    if (!this.#active) return;
    if (this.#queue.length >= this.options.maxPendingEvents) {
      this.close();
      this.fail(kernelError("LISTENER_DELIVERY_FAILED", "Listener queue capacity exceeded"));
      return;
    }
    this.#queue.push(event);
    this.scheduleDrain();
  }

  close(): void {
    if (!this.#active) return;
    this.#active = false;
    this.#queue.length = 0;
    this.#current?.abort();
  }

  private scheduleDrain(): void {
    if (this.#draining) return;
    this.#draining = true;
    queueMicrotask(() => void this.drain());
  }

  private async drain(): Promise<void> {
    while (this.#active) {
      const event = this.#queue.shift();
      if (!event) break;
      const controller = new AbortController();
      this.#current = controller;
      const outcome = await deliver(
        this.listener,
        event,
        controller.signal,
        this.options.timeoutMs,
      );
      if (this.#current === controller) this.#current = undefined;
      if (outcome === "rejected" || outcome === "timed_out") {
        this.close();
        this.fail(
          kernelError(
            "LISTENER_DELIVERY_FAILED",
            outcome === "rejected" ? "Listener delivery rejected" : "Listener delivery timed out",
          ),
        );
        break;
      }
    }
    this.#draining = false;
    if (this.#active && this.#queue.length > 0) this.scheduleDrain();
  }
}

function deliver(
  listener: KernelEventListener,
  event: KernelEvent,
  signal: AbortSignal,
  timeoutMs: number,
): Promise<DeliveryOutcome> {
  const invocation = Promise.resolve()
    .then(() => {
      if (!signal.aborted) return listener(event);
    })
    .then((): DeliveryOutcome => "delivered")
    .catch((): DeliveryOutcome => "rejected");
  return new Promise((resolve) => {
    let settled = false;
    const finish = (outcome: DeliveryOutcome): void => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal.removeEventListener("abort", onAbort);
      resolve(outcome);
    };
    const onAbort = (): void => {
      finish("cancelled");
    };
    const timer = setTimeout(() => {
      finish("timed_out");
    }, timeoutMs);
    signal.addEventListener("abort", onAbort, { once: true });
    if (signal.aborted) onAbort();
    void invocation.then(finish);
  });
}
