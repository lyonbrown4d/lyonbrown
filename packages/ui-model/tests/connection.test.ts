import { describe, expect, test } from "vitest";

import {
  createInitialUiModelState,
  getChatSubmitAvailability,
  getConnectionFreshness,
  getDaemonMutationAvailability,
  reduceUiModel,
  type RemoteSnapshot,
  type UiModelState,
} from "@lyonbrown/ui-model/internal/index";

function snapshot(
  serverInstanceId: string,
  revision: bigint,
  sessionId = "session-a",
): RemoteSnapshot {
  return {
    serverInstanceId,
    revision,
    sessionId,
    modelSetup: null,
    activities: [],
  };
}

function readyState(revision = 4n): UiModelState {
  let state = createInitialUiModelState();
  state = reduceUiModel(state, { type: "connectRequested" });
  state = reduceUiModel(state, {
    type: "handshakeSucceeded",
    serverInstanceId: "epoch-a",
  });
  return reduceUiModel(state, {
    type: "snapshotReceived",
    snapshot: snapshot("epoch-a", revision),
  });
}

describe("connection projection", () => {
  test("accepts only increasing revisions within one epoch", () => {
    const state = readyState();
    const duplicate = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-a", 4n, "duplicate"),
    });
    const older = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-a", 3n, "older"),
    });
    const newer = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-a", 5n, "newer"),
    });

    expect(duplicate).toBe(state);
    expect(older).toBe(state);
    expect(newer.remote.snapshot?.sessionId).toBe("newer");
    expect(newer.remote.snapshot?.revision).toBe(5n);
  });

  test("allows a lower revision after an epoch change but stays synchronizing until it arrives", () => {
    const previous = readyState(100n);
    const synchronizing = reduceUiModel(previous, {
      type: "handshakeSucceeded",
      serverInstanceId: "epoch-b",
    });

    expect(synchronizing.remote.connection).toBe("synchronizing");
    expect(synchronizing.remote.snapshotFresh).toBe(false);
    expect(getConnectionFreshness(synchronizing)).toEqual({
      enabled: false,
      reason: "synchronizing",
    });

    const ready = reduceUiModel(synchronizing, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-b", 1n, "session-b"),
    });
    expect(ready.remote.connection).toBe("ready");
    expect(ready.remote.snapshotFresh).toBe(true);
    expect(ready.remote.snapshot?.revision).toBe(1n);
    expect(getConnectionFreshness(ready)).toEqual({ enabled: true });
  });

  test("accepts an equal revision as freshness confirmation after reconnecting to the same epoch", () => {
    const previous = readyState(9n);
    const synchronizing = reduceUiModel(previous, {
      type: "handshakeSucceeded",
      serverInstanceId: "epoch-a",
    });
    const confirmed = reduceUiModel(synchronizing, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-a", 9n, "conflicting-session-at-same-revision"),
    });

    expect(confirmed.remote.connection).toBe("ready");
    expect(confirmed.remote.snapshotFresh).toBe(true);
    expect(confirmed.remote.snapshot?.revision).toBe(9n);
    expect(confirmed.remote.snapshot?.sessionId).toBe("session-a");
  });

  test("retains a readable snapshot on disconnect and disables every mutation", () => {
    const ready = readyState();
    const stale = reduceUiModel(ready, {
      type: "connectionLost",
      reason: "transport-closed",
    });

    expect(stale.remote.connection).toBe("stale");
    expect(stale.remote.snapshot).toBe(ready.remote.snapshot);
    expect(stale.remote.snapshotFresh).toBe(false);
    expect(getConnectionFreshness(stale)).toEqual({
      enabled: false,
      reason: "stale-snapshot",
    });

    const explicitlyDisconnected = reduceUiModel(ready, { type: "clientDisconnected" });
    expect(explicitlyDisconnected.remote.connection).toBe("stale");
    expect(explicitlyDisconnected.remote.snapshot).toBe(ready.remote.snapshot);
  });

  test("rejects snapshots that do not belong to the current handshake epoch", () => {
    const state = readyState();
    const mismatched = reduceUiModel(state, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-other", 99n),
    });

    expect(mismatched).toBe(state);
  });

  test("uses typed disable reasons for unavailable and disconnected clients", () => {
    const disconnected = createInitialUiModelState();
    const unavailable = reduceUiModel(disconnected, {
      type: "connectionUnavailable",
      reason: "daemon-not-running",
    });

    expect(getDaemonMutationAvailability(disconnected)).toEqual({
      enabled: false,
      reason: "disconnected",
    });
    expect(getChatSubmitAvailability(unavailable)).toEqual({
      enabled: false,
      reason: "unavailable",
    });
  });

  test.each([
    ["connecting", (state: UiModelState) => reduceUiModel(state, { type: "connectRequested" })],
    [
      "stale",
      (state: UiModelState) =>
        reduceUiModel(state, { type: "connectionLost", reason: "transport-closed" }),
    ],
    [
      "unavailable",
      (state: UiModelState) =>
        reduceUiModel(state, { type: "connectionUnavailable", reason: "daemon-unavailable" }),
    ],
    [
      "client-disconnected",
      (state: UiModelState) => reduceUiModel(state, { type: "clientDisconnected" }),
    ],
  ] as const)("does not let a late snapshot revive %s state", (_name, transition) => {
    const inactive = transition(readyState());
    const late = reduceUiModel(inactive, {
      type: "snapshotReceived",
      snapshot: snapshot("epoch-a", 99n, "late-session"),
    });

    expect(late).toBe(inactive);
    expect(late.remote.snapshotFresh).toBe(false);
  });
});
