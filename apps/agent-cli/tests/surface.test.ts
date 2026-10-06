import { describe, expect, it } from "vitest";
import { THEME_IDS, createInitialUiModelState } from "@lyonbrown/ui-model";

import { renderFrame } from "../src/surface.ts";
import { THEMES } from "../src/themes.ts";
import { runtimeSnapshot } from "./support/runtime.ts";

describe("TUI surface", () => {
  it("defines the six selectable palettes with a Claude Code leaning default", () => {
    expect(Object.keys(THEMES)).toEqual(THEME_IDS);
    expect(createInitialUiModelState().preferences.themeId).toBe("hearth-teal");
  });

  it("renders bilingual chrome, model progress, activity and approval", () => {
    const ui = createInitialUiModelState();
    const lines = renderFrame(
      {
        ui: {
          ...ui,
          remote: { ...ui.remote, connection: "connecting" },
          preferences: { ...ui.preferences, locale: "zh-CN" },
        },
        snapshot: runtimeSnapshot({
          phase: "awaiting-approval",
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
            downloadedBytes: 50n,
            totalBytes: 100n,
            failure: null,
            allowedActions: ["cancel"],
          },
          pendingApproval: {
            requestId: "approval-1",
            toolName: "write",
            argumentsJson: "{}",
            stepId: "step-1",
            preview: null,
            deadlineRemainingMs: 5_000n,
            state: "pending",
            approvedBy: null,
          },
        }),
        commandPaletteOpen: true,
        activityOpen: true,
        notice: null,
      },
      100,
    ).join("\n");

    expect(lines).toContain("正在连接");
    expect(lines).toContain("50%");
    expect(lines).toContain("需要批准");
    expect(lines).toContain("命令面板");
    expect(lines).toContain("活动检查器");
  });

  it("removes terminal control sequences from daemon-provided text", () => {
    const rendered = renderFrame(
      {
        ui: createInitialUiModelState(),
        snapshot: runtimeSnapshot({
          turns: [
            {
              turnId: "turn-1",
              userText: "hello\u001b]52;c;stolen\u0007",
              assistantText: "safe\u001b[2Jtext",
              status: "completed",
              steering: [],
              activities: [],
            },
          ],
        }),
        commandPaletteOpen: false,
        activityOpen: false,
        notice: null,
      },
      80,
    ).join("\n");

    expect(rendered).not.toContain("\u001b]52");
    expect(rendered).not.toContain("\u001b[2J");
    expect(rendered).toContain("safetext");
  });

  it("renders activation progress when there is no download operation", () => {
    const rendered = renderFrame(
      {
        ui: createInitialUiModelState(),
        snapshot: runtimeSnapshot({
          phase: "model-setup-required",
          modelSetup: {
            source: "managed-preset",
            phase: "activating",
            managedModel: null,
            operationId: null,
            downloadedBytes: 0n,
            totalBytes: null,
            failure: null,
            allowedActions: [],
          },
        }),
        commandPaletteOpen: false,
        activityOpen: false,
        notice: null,
      },
      80,
    ).join("\n");

    expect(rendered).toContain("activating");
  });
});
