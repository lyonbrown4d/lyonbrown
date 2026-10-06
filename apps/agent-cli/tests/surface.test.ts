import { describe, expect, it } from "vitest";
import { THEME_IDS, createInitialUiModelState } from "@lyonbrown/ui-model";

import { renderFrame } from "../src/surface.ts";
import { THEMES } from "../src/themes.ts";
import { runtimeSnapshot } from "./support/runtime.ts";

describe("TUI surface", () => {
  it("defines the accepted semantic tokens for all six themes with hearth-teal as default", () => {
    expect(Object.keys(THEMES)).toEqual(THEME_IDS);
    expect(createInitialUiModelState().preferences.themeId).toBe("hearth-teal");
    expect(THEMES["hearth-teal"]).toEqual({
      background: "#0B1113",
      surfaceRaised: "#122024",
      surfaceActive: "#182A2F",
      textPrimary: "#EAE7DF",
      textSecondary: "#A8A39A",
      textFaint: "#716D66",
      accentPrimary: "#E07A5F",
      accentSecondary: "#74C7B8",
      statusSuccess: "#6FCF97",
      statusWarning: "#E9B949",
      statusDanger: "#E76F76",
      borderDefault: "#2A3A3D",
      focusRing: "#E07A5F",
    });
    for (const theme of Object.values(THEMES)) {
      expect(Object.keys(theme)).toEqual([
        "background",
        "surfaceRaised",
        "surfaceActive",
        "textPrimary",
        "textSecondary",
        "textFaint",
        "accentPrimary",
        "accentSecondary",
        "statusSuccess",
        "statusWarning",
        "statusDanger",
        "borderDefault",
        "focusRing",
      ]);
    }
  });

  it.each([
    ["en-US", "Recommended for this hardware", "Enter/D  Download", "Q/Esc  Exit"],
    ["zh-CN", "根据当前硬件推荐", "Enter/D  下载", "Q/Esc  退出"],
  ] as const)(
    "renders an explicit %s recommended-model decision without starting a download",
    (locale, recommendation, download, exit) => {
      const ui = freshUi();
      const rendered = renderFrame(
        {
          ui: { ...ui, preferences: { ...ui.preferences, locale } },
          snapshot: runtimeSnapshot({
            phase: "model-setup-required",
            modelSetup: {
              source: "managed-preset",
              phase: "required",
              managedModel: {
                id: "qwen3-1.7b-q4_k_m",
                displayName: "Qwen3 1.7B Q4_K_M",
                recommendationReason: "hardware-compatible",
                expectedDownloadBytes: 1_107_408_544n,
                maxBytes: 2_147_483_648n,
                minAvailableMemoryBytes: 4_294_967_296n,
                sha256: "a".repeat(64),
              },
              operationId: null,
              downloadedBytes: 0n,
              totalBytes: null,
              failure: null,
              allowedActions: ["download"],
            },
          }),
          commandPaletteOpen: false,
          activityOpen: false,
          notice: null,
        },
        100,
      ).join("\n");

      expect(rendered).toContain("Qwen3 1.7B Q4_K_M");
      expect(rendered).toContain("qwen3-1.7b-q4_k_m");
      expect(rendered).toContain(recommendation);
      expect(rendered).toContain("1.03 GiB");
      expect(rendered).toContain(download);
      expect(rendered).toContain(exit);
    },
  );

  it("keeps model phase, progress, operation and actions legible at wide, medium and narrow widths", () => {
    const ui = freshUi();
    const frame = {
      ui,
      snapshot: runtimeSnapshot({
        phase: "model-setup-required",
        modelSetup: {
          source: "managed-preset" as const,
          phase: "downloading" as const,
          managedModel: {
            id: "qwen3-1.7b-q4_k_m",
            displayName: null,
            recommendationReason: null,
            expectedDownloadBytes: 100n * 1024n * 1024n,
            maxBytes: 110n * 1024n * 1024n,
            minAvailableMemoryBytes: 1n,
            sha256: "a".repeat(64),
          },
          operationId: "download-1",
          downloadedBytes: 50n * 1024n * 1024n,
          totalBytes: 100n * 1024n * 1024n,
          failure: null,
          allowedActions: ["cancel" as const],
        },
      }),
      commandPaletteOpen: false,
      activityOpen: false,
      notice: null,
    };

    for (const width of [100, 52, 30]) {
      const rendered = renderFrame(frame, width).join("\n");
      expect(rendered).toContain("Downloading");
      expect(rendered).toContain("50%");
      expect(rendered).toContain(width >= 52 ? "50 MiB / 100 MiB" : "50/100 MiB");
      expect(rendered).toContain("C/Esc");
      if (width >= 52) expect(rendered).toContain("download-1");
    }
  });

  it("renders a recoverable message below the supported terminal width", () => {
    const rendered = renderFrame(
      {
        ui: createInitialUiModelState(),
        snapshot: null,
        commandPaletteOpen: false,
        activityOpen: false,
        notice: null,
      },
      29,
    ).join("\n");

    expect(rendered).toContain("Terminal too small");
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
              displayName: null,
              recommendationReason: null,
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

    expect(rendered).toContain("Activating");
  });
});

function freshUi() {
  const ui = createInitialUiModelState();
  return {
    ...ui,
    remote: {
      ...ui.remote,
      connection: "ready" as const,
      snapshotFresh: true,
    },
  };
}
