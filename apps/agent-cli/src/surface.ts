import {
  parseColor,
  styleText,
  truncateToWidth,
  visibleWidth,
  wrapTextWithAnsi,
} from "@earendil-works/pi-tui";
import { basename } from "node:path";

import { renderApproval } from "@lyonbrown/agent-cli/internal/approval-surface";
import type { AgentTuiFrame } from "@lyonbrown/agent-cli/internal/controller";
import { labels } from "@lyonbrown/agent-cli/internal/locales";
import { renderModelSetup } from "@lyonbrown/agent-cli/internal/model-setup-surface";
import { safeTerminalText } from "@lyonbrown/agent-cli/internal/text-safety";
import { THEMES } from "@lyonbrown/agent-cli/internal/themes";

export interface FrameViewState {
  readonly approvalSelection?: number;
  readonly height?: number;
}

export function renderFrame(
  frame: AgentTuiFrame,
  width: number,
  view: FrameViewState = {},
): string[] {
  const safeWidth = Math.max(1, width);
  const locale = frame.ui.preferences.locale;
  const text = labels(locale);
  const palette = THEMES[frame.ui.preferences.themeId];
  const accent = (value: string): string =>
    styleText(value, { fg: parseColor(palette.accentPrimary), bold: true }, "truecolor");
  const secondary = (value: string): string =>
    styleText(value, { fg: parseColor(palette.accentSecondary) }, "truecolor");
  const muted = (value: string): string =>
    styleText(value, { fg: parseColor(palette.textSecondary), dim: true }, "truecolor");
  const success = (value: string): string =>
    styleText(value, { fg: parseColor(palette.statusSuccess), bold: true }, "truecolor");
  const warning = (value: string): string =>
    styleText(value, { fg: parseColor(palette.statusWarning), bold: true }, "truecolor");
  const danger = (value: string): string =>
    styleText(value, { fg: parseColor(palette.statusDanger), bold: true }, "truecolor");
  const active = (value: string): string =>
    styleText(
      value,
      {
        fg: parseColor(palette.textPrimary),
        bg: parseColor(palette.surfaceActive),
        bold: true,
      },
      "truecolor",
    );
  const styles = { accent, secondary, muted, success, warning, danger, active };
  if (width < 30) return [fit(warning(text.terminalTooSmall), safeWidth)];
  if ((view.height ?? Number.MAX_SAFE_INTEGER) < 8) {
    return [fit(warning(text.terminalTooShort), safeWidth)];
  }
  const setup = frame.snapshot?.modelSetup;
  if (
    setup !== null &&
    setup !== undefined &&
    setup.phase !== "ready" &&
    (view.height ?? Number.MAX_SAFE_INTEGER) <= 8
  ) {
    const mutationsEnabled =
      frame.ui.remote.connection === "ready" && frame.ui.remote.snapshotFresh;
    return [
      fit(
        renderContext(
          frame,
          text.connection[frame.ui.remote.connection],
          safeWidth,
          accent,
          secondary,
          muted,
        ),
        safeWidth,
      ),
      ...renderModelSetup({
        setup,
        locale,
        width: safeWidth,
        mutationsEnabled,
        styles: { heading: accent, muted, warning, danger },
      }).map((line) => fit(line, safeWidth)),
      fit(muted(text.footer), safeWidth),
    ];
  }
  const lines: string[] = [
    fit(
      renderContext(
        frame,
        text.connection[frame.ui.remote.connection],
        safeWidth,
        accent,
        secondary,
        muted,
      ),
      safeWidth,
    ),
    muted("─".repeat(safeWidth)),
  ];

  const turns = frame.snapshot?.turns ?? [];
  if (turns.length === 0) lines.push(muted(text.empty));
  for (const turn of turns) {
    lines.push(...renderTranscriptMessage(text.user, turn.userText, safeWidth, accent));
    if (turn.assistantText.length > 0) {
      lines.push(
        ...renderTranscriptMessage(text.assistant, turn.assistantText, safeWidth, accent, true),
      );
    }
    for (const activity of turn.activities) {
      const marker = activity.phase === "completed" ? success("✓") : secondary("▾");
      lines.push(
        fit(
          `  ${marker} ${safeTerminalText(activity.stepId)} · ${safeTerminalText(activity.phase)} · ${String(activity.tools.length)} tools`,
          safeWidth,
        ),
      );
    }
  }

  const queued = frame.snapshot?.queuedFollowUps ?? [];
  for (const message of queued) {
    lines.push(
      fit(
        `${secondary("☷")} ${locale === "zh-CN" ? "下一步队列" : "Next queue"}: ${safeTerminalText(message)}`,
        safeWidth,
      ),
    );
  }

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
      ...renderApproval(approval, locale, styles, view.approvalSelection).map((line) =>
        fit(line, safeWidth),
      ),
    );
  }
  if (frame.notice !== null) lines.push(fit(warning(safeTerminalText(frame.notice)), safeWidth));
  if (
    (setup === null || setup === undefined || setup.phase === "ready") &&
    (frame.snapshot?.pendingApproval === null || frame.snapshot?.pendingApproval === undefined)
  ) {
    lines.push(
      muted(
        isRunning(frame.snapshot?.phase)
          ? locale === "zh-CN"
            ? "Enter 立即引导 · Alt-Enter 排队到下一步"
            : "Enter steer now · Alt-Enter queue for next step"
          : locale === "zh-CN"
            ? "Enter 发送 · Alt-Enter 排队到下一步"
            : "Enter send · Alt-Enter queue for next step",
      ),
    );
  }
  lines.push(muted(text.footer));
  return lines;
}

function renderContext(
  frame: AgentTuiFrame,
  connection: string,
  width: number,
  accent: (value: string) => string,
  secondary: (value: string) => string,
  muted: (value: string) => string,
): string {
  const snapshot = frame.snapshot;
  const workspace = safeTerminalText(basename(process.cwd()));
  const session = snapshot?.sessionId ?? "—";
  const selected = snapshot?.route?.selected;
  const model =
    selected === undefined
      ? snapshot?.modelSelection.kind === "pinned"
        ? `${snapshot.modelSelection.provider}/${snapshot.modelSelection.model}`
        : "auto"
      : `${selected.provider}/${selected.model}`;
  const mode = snapshot?.modelSelection.kind === "pinned" ? "MANUAL" : "AUTO";
  const workspaceLabel = width < 110 ? workspace : `workspace: ${workspace}`;
  const sessionLabel = width < 110 ? "" : `  │  ${muted(`session: ${session}`)}`;
  return `${accent("LYONBROWN")}  ${muted(connection)}  │  ${muted(workspaceLabel)}${sessionLabel}  │  ${accent(mode)}  │  ${secondary(model)}`;
}

function isRunning(phase: NonNullable<AgentTuiFrame["snapshot"]>["phase"] | undefined): boolean {
  return phase === "preparing" || phase === "streaming" || phase === "running-tool";
}

function renderTranscriptMessage(
  label: string,
  value: string,
  width: number,
  accent: (value: string) => string,
  trimBlankEdges = false,
): string[] {
  const plainLabel = `${label}:`;
  const indent = " ".repeat(visibleWidth(plainLabel) + 1);
  let wrapped = wrapTextWithAnsi(
    safeTerminalText(value),
    Math.max(1, width - visibleWidth(indent)),
  );
  if (trimBlankEdges) {
    while (wrapped[0]?.trim().length === 0) wrapped = wrapped.slice(1);
    while (wrapped.at(-1)?.trim().length === 0) wrapped = wrapped.slice(0, -1);
  }
  return wrapped.map((line, index) =>
    fit(`${index === 0 ? `${accent(plainLabel)} ` : indent}${line}`, width),
  );
}

function fit(value: string, width: number): string {
  return truncateToWidth(value, width, "");
}
