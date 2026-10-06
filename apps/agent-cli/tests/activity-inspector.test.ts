import { describe, expect, it } from "vitest";

import { renderActivityInspector } from "@lyonbrown/agent-cli/internal/activity-surface";
import { runtimeSnapshot } from "@lyonbrown/agent-cli/test/support/runtime";

const styles = {
  accent: String,
  secondary: String,
  muted: String,
  success: String,
  warning: String,
  danger: String,
  active: (value: string) => `[${value}]`,
};

const snapshot = runtimeSnapshot({
  turns: [
    {
      turnId: "turn-1",
      userText: "inspect",
      assistantText: "",
      status: "running",
      steering: [],
      activities: [
        {
          stepId: "step-1",
          phase: "completed",
          route: {
            selected: { provider: "local", model: "small" },
            reason: "first route",
            fallbacks: [],
          },
          context: null,
          assistantText: "first output",
          usage: null,
          tools: [],
          outcome: null,
        },
        {
          stepId: "step-2",
          phase: "running-tool",
          route: {
            selected: { provider: "remote", model: "large" },
            reason: "second route",
            fallbacks: [],
          },
          context: null,
          assistantText: "second output",
          usage: null,
          tools: [],
          outcome: null,
        },
      ],
    },
  ],
});

describe("responsive Activity Inspector", () => {
  it("uses list and detail columns at 90+ columns and honors the selected activity", () => {
    const rendered = renderActivityInspector(snapshot, "en-US", true, styles, {
      width: 100,
      selection: 0,
      narrowPane: "list",
      detailOffset: 0,
    }).join("\n");

    expect(rendered).toContain("[▸] step-1");
    expect(rendered).toContain("first route");
    expect(rendered).not.toContain("second route");
    expect(rendered).toContain("│");
  });

  it("shows one pane at 30 columns and switches to bounded detail", () => {
    const list = renderActivityInspector(snapshot, "en-US", true, styles, {
      width: 30,
      selection: 1,
      narrowPane: "list",
      detailOffset: 0,
    }).join("\n");
    const detail = renderActivityInspector(snapshot, "en-US", true, styles, {
      width: 30,
      selection: 1,
      narrowPane: "detail",
      detailOffset: 0,
    }).join("\n");

    expect(list).toContain("step-2");
    expect(list).not.toContain("second route");
    expect(detail).toContain("second route");
    expect(detail).not.toContain("step-1 · completed");
  });
});
