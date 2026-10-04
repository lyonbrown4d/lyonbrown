import { describe, expect, it } from "vitest";

import { projectRemoteSnapshot } from "../src/projection.js";
import { runtimeSnapshot } from "./support/runtime.js";

describe("runtime projection", () => {
  it("projects model download and activity state into the shared UI model", () => {
    const projected = projectRemoteSnapshot(
      "11111111-1111-4111-8111-111111111111",
      runtimeSnapshot({
        revision: 9n,
        phase: "streaming",
        modelSetup: {
          source: "managed-preset",
          phase: "downloading",
          managedModel: {
            id: "qwen3-1.7b-q4_k_m",
            expectedDownloadBytes: 100n,
            maxBytes: 110n,
            minAvailableMemoryBytes: 1n,
            sha256: "a".repeat(64),
          },
          operationId: "download-1",
          downloadedBytes: 25n,
          totalBytes: 100n,
          failure: null,
          allowedActions: ["cancel"],
        },
        turns: [
          {
            turnId: "turn-1",
            userText: "hi",
            assistantText: "hello",
            status: "running",
            steering: [],
            activities: [
              {
                stepId: "step-1",
                phase: "streaming",
                route: null,
                context: null,
                assistantText: "hello",
                usage: null,
                tools: [],
                outcome: null,
              },
            ],
          },
        ],
      }),
    );

    expect(projected.modelSetup).toEqual({ phase: "Downloading", operationId: "download-1" });
    expect(projected.activities).toEqual([
      { turnId: "turn-1", stepId: "step-1", kind: "streaming" },
    ]);
  });
});
