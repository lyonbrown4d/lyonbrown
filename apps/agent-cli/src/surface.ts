import { parseColor, styleText, truncateToWidth } from "@earendil-works/pi-tui";

import type { AgentTuiFrame } from "./controller.ts";
import { labels } from "./locales.ts";
import { renderModelSetup } from "./model-setup-surface.ts";
import { safeTerminalText } from "./text-safety.ts";
import { THEMES } from "./themes.ts";

const COMMANDS = [
  "/language [en|zh]",
  "/theme <name>",
  "/download · /retry · /cancel",
  "/steer <text> · /follow-up <text>",
  "/approve once|session · /deny once|remaining",
  "/activity · /commands",
] as const;

export function renderFrame(frame: AgentTuiFrame, width: number): string[] {
  const safeWidth = Math.max(1, width);
  const locale = frame.ui.preferences.locale;
  const text = labels(locale);
  const palette = THEMES[frame.ui.preferences.themeId];
  const accent = (value: string): string =>
    styleText(value, { fg: parseColor(palette.accentPrimary), bold: true }, "truecolor");
  const muted = (value: string): string =>
    styleText(value, { fg: parseColor(palette.textSecondary), dim: true }, "truecolor");
  const warning = (value: string): string =>
    styleText(value, { fg: parseColor(palette.statusWarning), bold: true }, "truecolor");
  const danger = (value: string): string =>
    styleText(value, { fg: parseColor(palette.statusDanger), bold: true }, "truecolor");
  if (width < 30) return [fit(warning(text.terminalTooSmall), safeWidth)];
  const lines: string[] = [
    fit(`${accent("Lyonbrown")}  ${muted(text.connection[frame.ui.remote.connection])}`, safeWidth),
    muted("─".repeat(safeWidth)),
  ];

  const turns = frame.snapshot?.turns ?? [];
  if (turns.length === 0) lines.push(muted(text.empty));
  for (const turn of turns) {
    lines.push(fit(`${accent(`${text.user}:`)} ${safeTerminalText(turn.userText)}`, safeWidth));
    if (turn.assistantText.length > 0) {
      for (const line of safeTerminalText(turn.assistantText).split("\n")) {
        lines.push(fit(`${accent(`${text.assistant}:`)} ${line}`, safeWidth));
      }
    }
  }

  const setup = frame.snapshot?.modelSetup;
  if (setup !== null && setup !== undefined && setup.phase !== "ready") {
    const mutationsEnabled =
      frame.ui.remote.connection === "ready" && frame.ui.remote.snapshotFresh;
    lines.push(
      ...renderModelSetup({
        setup,
        locale,
        width: safeWidth,
        mutationsEnabled,
        styles: { heading: accent, muted, warning, danger },
      }).map((line) => fit(line, safeWidth)),
    );
  }

  const approval = frame.snapshot?.pendingApproval;
  if (approval !== null && approval !== undefined) {
    lines.push(
      fit(`${warning(text.approval)} · ${safeTerminalText(approval.toolName)}`, safeWidth),
    );
  }
  if (frame.commandPaletteOpen) {
    lines.push(accent(text.commands));
    lines.push(...COMMANDS.map((command) => fit(`  ${command}`, safeWidth)));
  }
  if (frame.activityOpen) {
    lines.push(accent(text.activity));
    const activities = frame.snapshot?.turns.flatMap((turn) => turn.activities) ?? [];
    if (activities.length === 0) lines.push(`  ${muted("—")}`);
    for (const activity of activities.slice(-8)) {
      lines.push(
        fit(
          `  ${safeTerminalText(activity.stepId)} · ${safeTerminalText(activity.phase)} · ${String(activity.tools.length)} tools`,
          safeWidth,
        ),
      );
    }
  }
  if (frame.notice !== null) lines.push(fit(warning(safeTerminalText(frame.notice)), safeWidth));
  lines.push(muted(text.footer));
  return lines;
}

function fit(value: string, width: number): string {
  return truncateToWidth(value, width, "");
}
