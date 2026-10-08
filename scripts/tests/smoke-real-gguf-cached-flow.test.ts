import { describe, expect, test, vi } from "vitest";

import {
  runRealGgufSmokeFlow,
  type SmokeSnapshot,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-flow";

const MODEL_ID = "qwen3-0.6b-q8_0";
const DIGEST = "a".repeat(64);

describe("real GGUF cached-model orchestration", () => {
  test("reuses a verified installed model without starting a download", async () => {
    let snapshot = readySnapshot("session-1", []);
    const startDownload = vi.fn(() => Promise.resolve());
    const verifyInstalledCache = vi.fn(() => Promise.resolve());
    const cleanup = vi.fn(() => Promise.resolve());
    const port = {
      readSnapshot: vi.fn(() => Promise.resolve(snapshot)),
      startDownload,
      cancelDownload: vi.fn(() => Promise.resolve()),
      startTurn: vi.fn((text: string) => {
        snapshot = text.startsWith("Respond")
          ? withTurn(snapshot, text, "completed")
          : text.startsWith("Call the write")
            ? withToolTurn(snapshot, text, "pending")
            : withTurn(snapshot, text, "running");
        return Promise.resolve();
      }),
      cancelTurn: vi.fn(() => {
        snapshot = updateLastTurn(snapshot, "cancelled");
        return Promise.resolve();
      }),
      respondApproval: vi.fn(() => {
        snapshot = completeToolTurn(snapshot);
        return Promise.resolve();
      }),
      selectModel: vi.fn((selection: SmokeSnapshot["modelSelection"]) => {
        snapshot = { ...snapshot, modelSelection: selection };
        return Promise.resolve();
      }),
      verifyWorkspaceFile: vi.fn(() => Promise.resolve()),
      closeServer: vi.fn(() => Promise.resolve()),
      restartServer: vi.fn(() => {
        snapshot = readySnapshot("session-2", []);
        return Promise.resolve();
      }),
      verifyInstalledCache,
      cleanup,
    };

    await runRealGgufSmokeFlow(port, {
      phaseTimeoutMs: 100,
      downloadTimeoutMs: 100,
      pollIntervalMs: 1,
      cancelAfterBytes: 64n,
      closeTimeoutMs: 100,
      trustedModelDigests: { [MODEL_ID]: DIGEST },
    });

    expect(startDownload).not.toHaveBeenCalled();
    expect(verifyInstalledCache).toHaveBeenCalledWith(DIGEST);
    expect(cleanup).toHaveBeenCalledOnce();
  });
});

function readySnapshot(sessionId: string, turns: SmokeSnapshot["turns"]): SmokeSnapshot {
  return {
    revision: 1n,
    phase: "ready",
    sessionId,
    pendingApproval: null,
    modelSelection: { kind: "auto" },
    modelSetup: {
      phase: "ready",
      operationId: null,
      downloadedBytes: 639_446_688n,
      totalBytes: 639_446_688n,
      managedModel: { id: MODEL_ID, maxBytes: 1_073_741_824n, sha256: DIGEST },
      allowedActions: [],
      failure: null,
    },
    turns,
  };
}

function withTurn(
  snapshot: SmokeSnapshot,
  userText: string,
  status: "running" | "completed",
): SmokeSnapshot {
  return {
    ...snapshot,
    phase: status === "running" ? "streaming" : "completed",
    turns: [
      ...snapshot.turns,
      {
        userText,
        status,
        assistantText: status === "running" ? "token" : "confirmed",
        activities: [
          {
            phase: status === "running" ? "streaming" : "completed",
            usage: status === "running" ? null : { input: 1, output: 1 },
            tools: [],
          },
        ],
      },
    ],
  };
}

function withToolTurn(snapshot: SmokeSnapshot, userText: string, status: "pending"): SmokeSnapshot {
  void status;
  const argumentsJson = '{"path":"lyonbrown-tool-proof.txt","content":"LYONBROWN_TOOL_OK"}';
  return {
    ...snapshot,
    phase: "awaiting-approval",
    pendingApproval: {
      requestId: "approval-1",
      toolName: "write",
      argumentsJson,
    },
    turns: [
      ...snapshot.turns,
      {
        userText,
        status: "running",
        assistantText: "",
        activities: [
          {
            phase: "awaiting-approval",
            usage: null,
            tools: [
              {
                name: "write",
                argumentsJson,
                state: "requested",
                result: null,
              },
            ],
          },
        ],
      },
    ],
  };
}

function completeToolTurn(snapshot: SmokeSnapshot): SmokeSnapshot {
  const turn = snapshot.turns.at(-1);
  if (turn === undefined) throw new Error("missing tool turn");
  return {
    ...snapshot,
    phase: "completed",
    pendingApproval: null,
    turns: [
      ...snapshot.turns.slice(0, -1),
      {
        ...turn,
        status: "completed",
        assistantText: "done",
        activities: turn.activities.map((activity) => ({
          ...activity,
          phase: "completed",
          usage: { input: 1, output: 1 },
          tools: activity.tools.map((tool) => ({
            ...tool,
            state: "completed",
            result: { ok: true, error: null },
          })),
        })),
      },
    ],
  };
}

function updateLastTurn(snapshot: SmokeSnapshot, status: "cancelled"): SmokeSnapshot {
  const turn = snapshot.turns.at(-1);
  if (turn === undefined) throw new Error("missing turn");
  return {
    ...snapshot,
    phase: "ready",
    turns: [...snapshot.turns.slice(0, -1), { ...turn, status }],
  };
}
