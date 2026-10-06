import { describe, expect, test, vi } from "vitest";

import { assertSmokeSnapshotHealthy } from "@lyonbrown/workspace/internal/smoke-real-gguf-assertions";
import {
  runRealGgufSmokeFlow,
  type SmokeSnapshot,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-flow";

describe("real GGUF failure fencing", () => {
  test("fails immediately when model setup reports a terminal failure", () => {
    const failed = smokeSnapshots()[0];
    const modelSetup = failed?.modelSetup;
    if (failed === undefined || modelSetup === null || modelSetup === undefined) {
      throw new Error("missing fixture");
    }
    expect(() => {
      assertSmokeSnapshotHealthy({
        ...failed,
        phase: "failed-recoverable",
        modelSetup: {
          ...modelSetup,
          phase: "failed",
          failure: { code: "network_error" },
        },
      });
    }).toThrow("REAL_GGUF_MODEL_SETUP_FAILED:network_error");
  });
});

describe("real GGUF smoke orchestration", () => {
  test("cancels a download, retries, chats, restarts from cache, and cleans up", async () => {
    const calls: string[] = [];
    const snapshots = smokeSnapshots();
    let index = 0;
    const nextSnapshot = (): SmokeSnapshot => {
      const snapshot = snapshots[Math.min(index++, snapshots.length - 1)];
      if (snapshot === undefined) throw new Error("missing fake snapshot");
      return snapshot;
    };
    const port = {
      readSnapshot: vi.fn(() => Promise.resolve(nextSnapshot())),
      startDownload: vi.fn((modelId: string) => {
        calls.push(`download:${modelId}`);
        return Promise.resolve();
      }),
      cancelDownload: vi.fn((operationId: string) => {
        calls.push(`cancel-download:${operationId}`);
        return Promise.resolve();
      }),
      startTurn: vi.fn((text: string) => {
        calls.push(text.startsWith("Respond") ? "short-turn" : "long-turn");
        return Promise.resolve();
      }),
      cancelTurn: vi.fn(() => {
        calls.push("cancel-turn");
        return Promise.resolve();
      }),
      closeServer: vi.fn(() => {
        calls.push("close-server");
        return Promise.resolve();
      }),
      restartServer: vi.fn(() => {
        calls.push("restart-server");
        return Promise.resolve();
      }),
      verifyInstalledCache: vi.fn(() => {
        calls.push("verify-cache");
        return Promise.resolve();
      }),
      cleanup: vi.fn(() => {
        calls.push("cleanup");
        return Promise.resolve();
      }),
    };

    await runRealGgufSmokeFlow(port, {
      phaseTimeoutMs: 1_000,
      downloadTimeoutMs: 1_000,
      pollIntervalMs: 1,
      cancelAfterBytes: 64n,
      closeTimeoutMs: 1_000,
      expectedSha256: "a".repeat(64),
    });

    expect(calls).toEqual([
      "download:qwen3-1.7b-q4_k_m",
      "cancel-download:download-1",
      "download:qwen3-1.7b-q4_k_m",
      "short-turn",
      "long-turn",
      "cancel-turn",
      "short-turn",
      "close-server",
      "verify-cache",
      "restart-server",
      "short-turn",
      "long-turn",
      "close-server",
      "cleanup",
    ]);
  });
});

function smokeSnapshots(): readonly SmokeSnapshot[] {
  const setup = (
    phase: NonNullable<SmokeSnapshot["modelSetup"]>["phase"],
    operationId: string | null,
    downloadedBytes: bigint,
    sessionId = "session-1",
  ): SmokeSnapshot => ({
    revision: 1n,
    phase: phase === "ready" ? "ready" : "model-setup-required",
    sessionId,
    modelSetup: {
      phase,
      operationId,
      downloadedBytes,
      totalBytes: phase === "required" ? null : 1_000n,
      managedModel: {
        id: "qwen3-1.7b-q4_k_m",
        maxBytes: 2_000n,
        sha256: "a".repeat(64),
      },
      allowedActions:
        phase === "required"
          ? ["download"]
          : phase === "cancelled"
            ? ["retry"]
            : phase === "downloading" || phase === "verifying"
              ? ["cancel"]
              : [],
      failure: null,
    },
    turns: [],
  });
  const turn = (
    count: number,
    status: "running" | "completed" | "cancelled",
    sessionId = "session-1",
  ): SmokeSnapshot => ({
    ...setup("ready", null, 1_000n, sessionId),
    phase: status === "running" ? "streaming" : status === "cancelled" ? "ready" : "completed",
    turns: Array.from({ length: count }, (_, index) => {
      const current = index + 1 === count;
      const isLong = current && status !== "completed";
      return {
        userText: isLong
          ? "Write a detailed numbered explanation with at least 500 items. Continue until cancelled."
          : "Respond with exactly one short sentence confirming the smoke test.",
        status: current ? status : "completed",
        assistantText: current && status !== "completed" ? "token" : "pong",
        activities: [
          {
            phase: !current || status === "completed" ? "completed" : "streaming",
            usage: !current || status === "completed" ? { input: 1, output: 1 } : null,
          },
        ],
      };
    }),
  });
  return [
    setup("required", null, 0n),
    setup("downloading", "download-1", 128n),
    setup("cancelled", "download-1", 128n),
    setup("downloading", "download-2", 256n),
    setup("downloading", "download-2", 900n),
    setup("verifying", "download-2", 0n),
    setup("ready", null, 1_000n),
    setup("ready", null, 1_000n),
    turn(1, "completed"),
    turn(1, "completed"),
    turn(2, "running"),
    turn(2, "cancelled"),
    turn(2, "cancelled"),
    turn(3, "completed"),
    setup("ready", null, 1_000n, "session-2"),
    setup("ready", null, 1_000n, "session-2"),
    turn(1, "completed", "session-2"),
    turn(1, "completed", "session-2"),
    turn(2, "running", "session-2"),
  ];
}
