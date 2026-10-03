import { describe, expect, test } from "vitest";

import {
  createInitialUiModelState,
  getChatSubmitAvailability,
  getDaemonMutationAvailability,
  getMutationAvailability,
  reduceUiModel,
  type RemoteSnapshot,
  type UiModelState,
} from "../src/index.js";

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
    expect(getMutationAvailability(synchronizing)).toEqual({
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
    expect(getMutationAvailability(ready)).toEqual({ enabled: true });
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
    expect(getMutationAvailability(stale)).toEqual({
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
});
