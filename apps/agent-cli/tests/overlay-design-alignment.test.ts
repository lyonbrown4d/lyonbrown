import { stripTerminalSequences } from "@earendil-works/pi-tui";
import { createInitialUiModelState } from "@lyonbrown/ui-model";
import { describe, expect, it } from "vitest";

import { renderOverlay } from "@lyonbrown/agent-cli/internal/overlay-surface";
import { renderFrame } from "@lyonbrown/agent-cli/internal/surface";
import { runtimeSnapshot } from "@lyonbrown/agent-cli/test/support/runtime";

describe("accepted TUI overlay design alignment", () => {
  it("distinguishes a temporary theme preview from the saved theme", () => {
    const ui = createInitialUiModelState();
    const frame = {
      ui: { ...ui, preferences: { ...ui.preferences, themeId: "slate-blue" as const } },
      snapshot: runtimeSnapshot(),
      overlay: { kind: "control" as const, tab: "theme" as const },
      notice: null,
    };

    const rendered = stripTerminalSequences(
      renderOverlay(frame, 92, {
        selection: 2,
        previewTheme: "slate-blue",
        savedTheme: "carbon-mint",
      }).join("\n"),
    );

    expect(rendered).toContain("Carbon Mint · 碳墨薄荷  [saved]");
    expect(rendered).toContain("Slate Blue · 岩板蓝  [previewing]");
    expect(rendered).not.toContain("Slate Blue · 岩板蓝  [saved");
  });

  it("keeps control-panel actions readable in a narrow overlay", () => {
    const ui = createInitialUiModelState();
    const rendered = stripTerminalSequences(
      renderOverlay(
        {
          ui,
          snapshot: runtimeSnapshot(),
          overlay: { kind: "control" as const, tab: "theme" as const },
          notice: null,
        },
        52,
      ).join("\n"),
    );

    expect(rendered).toContain("Space preview");
    expect(rendered).toContain("Enter apply and remember");
    expect(rendered).toContain("Esc close");
    expect(rendered).toContain("Theme: Hearth Teal · 炉火深青");
  });

  it("localizes runtime status and overlay instructions", () => {
    const ui = createInitialUiModelState();
    const rendered = stripTerminalSequences(
      renderOverlay(
        {
          ui: { ...ui, preferences: { ...ui.preferences, locale: "zh-CN" as const } },
          snapshot: runtimeSnapshot(),
          overlay: { kind: "status" as const },
          notice: null,
        },
        80,
      ).join("\n"),
    );

    expect(rendered).toContain("运行状态");
    expect(rendered).toContain("连接");
    expect(rendered).toContain("会话");
    expect(rendered).toContain("Esc  关闭");
    expect(rendered).not.toContain("Runtime status");
  });

  it("renders the complete model, theme and language control panel in both locales", () => {
    const ui = createInitialUiModelState();
    const snapshot = runtimeSnapshot({
      modelCatalog: {
        models: [
          {
            route: { provider: "local", model: "qwen3-4b" },
            tier: "worker",
            locality: "local",
            contextWindow: 32_768,
            maxOutputTokens: 4_096,
            supportsTools: true,
            supportsStreaming: true,
            supportsJson: true,
            supportsThinking: true,
            selectableForWorker: true,
            disabledReason: null,
          },
        ],
      },
      modelPolicy: { active: { kind: "auto" }, pending: null },
    });
    const frame = {
      ui: { ...ui, preferences: { ...ui.preferences, locale: "zh-CN" as const } },
      snapshot,
      overlay: { kind: "control" as const, tab: "theme" as const },
      notice: null,
    };

    const rendered = stripTerminalSequences(renderOverlay(frame, 92).join("\n"));

    expect(rendered).toContain("控制面板");
    expect(rendered).toContain("模型");
    expect(rendered).toContain("主题");
    expect(rendered).toContain("语言");
    expect(rendered).toContain("local/qwen3-4b");
    expect(rendered).toContain("Carbon Mint");
    expect(rendered).toContain("Graphite Coral");
    expect(rendered).toContain("炉火深青");
    expect(rendered).toContain("简体中文");
    expect(rendered).toContain("English");
    expect(rendered).toContain("Space 预览");
    expect(rendered).toContain("Enter 应用并记住");
  });

  it("renders the accepted command catalog and activity evidence instead of summary-only rows", () => {
    const frame = {
      ui: createInitialUiModelState(),
      snapshot: runtimeSnapshot({
        turns: [
          {
            turnId: "turn-1",
            userText: "Inspect the repository",
            assistantText: "Working on it.",
            status: "running",
            steering: [],
            activities: [
              {
                stepId: "step-1",
                phase: "running-tool",
                route: {
                  selected: { provider: "local", model: "qwen3-4b" },
                  reason: "hardware compatible",
                  fallbacks: [],
                },
                context: { systemTokens: 120, contextTokens: 240, dropped: 0 },
                assistantText: "Inspecting files",
                usage: { input: 300, output: 42, cacheRead: 100 },
                tools: [
                  {
                    callId: "call-1",
                    name: "read",
                    argumentsJson: '{"path":"AGENTS.md"}',
                    state: "completed",
                    approvedBy: null,
                    result: {
                      invocationId: "invoke-1",
                      ok: true,
                      output: "Node.js / TypeScript",
                      truncated: false,
                      durationMs: 18n,
                      error: null,
                    },
                  },
                ],
                outcome: null,
              },
            ],
          },
        ],
      }),
      overlay: null,
      notice: null,
    };
    const rendered = stripTerminalSequences(
      [
        ...renderFrame(frame, 120),
        ...renderOverlay({ ...frame, overlay: { kind: "commands" } }, 100),
        ...renderOverlay({ ...frame, overlay: { kind: "activity" } }, 100),
      ].join("\n"),
    );

    for (const command of [
      "/model",
      "/theme",
      "/language",
      "/sessions",
      "/new",
      "/resume",
      "/status",
      "/queue",
      "/help",
      "/quit",
    ]) {
      expect(rendered).toContain(command);
    }
    expect(rendered).toContain("hardware compatible");
    expect(rendered).toContain("300 input");
    expect(rendered).toContain("100 cache");
    expect(rendered).toContain("AGENTS.md");
    expect(rendered).toContain("Node.js / TypeScript");
  });
});
