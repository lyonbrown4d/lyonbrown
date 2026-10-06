import { describe, expect, test, vi } from "vitest";

import { ListenerDispatcher } from "@lyonbrown/kernel-pi/internal/listener-dispatcher";
import type { KernelEvent } from "@lyonbrown/kernel-pi/internal/types";

const event = (turn: number): KernelEvent => ({ type: "turn_started", turn });

describe("listener dispatcher", () => {
  test("delivers each subscriber in order without blocking dispatch", async () => {
    const delivered: number[] = [];
    const releases: (() => void)[] = [];
    const dispatcher = new ListenerDispatcher({ maxPendingEvents: 8, timeoutMs: 1_000 });
    dispatcher.subscribe(
      (item) =>
        new Promise<void>((resolve) => {
          delivered.push((item as { turn: number }).turn);
          releases.push(resolve);
        }),
    );

    dispatcher.dispatch(event(1));
    dispatcher.dispatch(event(2));
    await vi.waitFor(() => {
      expect(delivered).toEqual([1]);
    });
    releases.shift()?.();
    await vi.waitFor(() => {
      expect(delivered).toEqual([1, 2]);
    });
    releases.shift()?.();
    dispatcher.dispose();
  });

  test("consumes rejection and automatically removes the subscriber", async () => {
    const diagnostics: string[] = [];
    const listener = vi.fn(() => Promise.reject(new Error("listener secret")));
    const dispatcher = new ListenerDispatcher({ maxPendingEvents: 8, timeoutMs: 1_000 }, (error) =>
      diagnostics.push(error.code),
    );

    dispatcher.subscribe(listener);
    dispatcher.dispatch(event(1));
    dispatcher.dispatch(event(2));
    await vi.waitFor(() => {
      expect(diagnostics).toEqual(["LISTENER_DELIVERY_FAILED"]);
    });
    dispatcher.dispatch(event(3));
    await Promise.resolve();

    expect(listener).toHaveBeenCalledOnce();
    dispatcher.dispose();
  });

  test("bounds a hung subscriber queue and supports unsubscribe cleanup", async () => {
    const diagnostics: string[] = [];
    const listener = vi.fn(() => new Promise<void>(() => undefined));
    const dispatcher = new ListenerDispatcher({ maxPendingEvents: 1, timeoutMs: 10_000 }, (error) =>
      diagnostics.push(error.message),
    );
    const unsubscribe = dispatcher.subscribe(listener);

    dispatcher.dispatch(event(1));
    await vi.waitFor(() => {
      expect(listener).toHaveBeenCalledOnce();
    });
    dispatcher.dispatch(event(2));
    dispatcher.dispatch(event(3));
    await vi.waitFor(() => {
      expect(diagnostics).toEqual(["Listener queue capacity exceeded"]);
    });
    unsubscribe();
    dispatcher.dispatch(event(4));
    expect(listener).toHaveBeenCalledOnce();
    dispatcher.dispose();
  });

  test("times out a hung subscriber and consumes its pending invocation", async () => {
    const diagnostics: string[] = [];
    const dispatcher = new ListenerDispatcher({ maxPendingEvents: 4, timeoutMs: 10 }, (error) =>
      diagnostics.push(error.message),
    );
    dispatcher.subscribe(() => new Promise<void>(() => undefined));
    dispatcher.dispatch(event(1));

    await vi.waitFor(() => {
      expect(diagnostics).toEqual(["Listener delivery timed out"]);
    });
    dispatcher.dispose();
  });
});
