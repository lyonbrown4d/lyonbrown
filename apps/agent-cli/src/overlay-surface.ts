import {
  backgroundAnsi,
  parseColor,
  styleText,
  truncateToWidth,
  visibleWidth,
} from "@earendil-works/pi-tui";
import { THEME_IDS, type ThemeId } from "@lyonbrown/ui-model";

import { renderActivityInspector } from "@lyonbrown/agent-cli/internal/activity-surface";
import { commandCatalog } from "@lyonbrown/agent-cli/internal/command-catalog";
import type { AgentTuiFrame } from "@lyonbrown/agent-cli/internal/controller";
import type { ControlPanelTab } from "@lyonbrown/agent-cli/internal/commands";
import { renderDecisionModel } from "@lyonbrown/agent-cli/internal/decision-model-surface";
import {
  renderHelpOverlay,
  renderSessionsOverlay,
  renderStatusOverlay,
} from "@lyonbrown/agent-cli/internal/informational-overlays";
import { safeTerminalText } from "@lyonbrown/agent-cli/internal/text-safety";
import { THEMES } from "@lyonbrown/agent-cli/internal/themes";

export interface OverlayViewState {
  readonly selection?: number;
  readonly query?: string;
  readonly controlTab?: ControlPanelTab;
  readonly previewTheme?: ThemeId | null;
  readonly savedTheme?: ThemeId;
  readonly activitySelection?: number;
  readonly activityPane?: "list" | "detail";
  readonly activityDetailOffset?: number;
}

const THEME_NAMES: Readonly<Record<ThemeId, readonly [string, string]>> = {
  "carbon-mint": ["Carbon Mint", "碳墨薄荷"],
  "ink-violet": ["Ink Violet", "墨夜紫罗兰"],
  "slate-blue": ["Slate Blue", "岩板蓝"],
  "hearth-teal": ["Hearth Teal", "炉火深青"],
  "sandstone-espresso": ["Sandstone Espresso", "砂岩咖啡"],
  "graphite-coral": ["Graphite Coral", "石墨珊瑚"],
};

export function renderOverlay(
  frame: AgentTuiFrame,
  width: number,
  view: OverlayViewState = {},
): string[] {
  const overlay = frame.overlay;
  if (overlay === null) return [];
  const locale = frame.ui.preferences.locale;
  const styles = overlayStyles(frame.ui.preferences.themeId);
  let body: string[];
  switch (overlay.kind) {
    case "commands":
      body = renderCommands(frame, view, styles);
      break;
    case "control":
      body = renderControl(frame, view.controlTab ?? overlay.tab, width, view, styles);
      break;
    case "activity":
      body = renderActivityInspector(
        frame.snapshot,
        locale,
        frame.ui.remote.snapshotFresh,
        styles,
        {
          width,
          selection: view.activitySelection ?? Number.MAX_SAFE_INTEGER,
          narrowPane: view.activityPane ?? "list",
          detailOffset: view.activityDetailOffset ?? 0,
        },
      );
      break;
    case "status":
      body = renderStatusOverlay(frame, styles);
      break;
    case "sessions":
      body = renderSessionsOverlay(frame, styles);
      break;
    case "help":
      body = renderHelpOverlay(frame, styles);
      break;
  }
  return box(body, Math.max(30, width), frame.ui.preferences.themeId);
}

function renderCommands(
  frame: AgentTuiFrame,
  view: OverlayViewState,
  styles: ReturnType<typeof overlayStyles>,
): string[] {
  const zh = frame.ui.preferences.locale === "zh-CN";
  const query = view.query?.toLocaleLowerCase() ?? "";
  const items = commandCatalog(frame.ui.preferences.locale).filter(
    (item) =>
      query.length === 0 ||
      `${item.command} ${item.description}`.toLocaleLowerCase().includes(query),
  );
  const selected = clamp(view.selection ?? 0, items.length);
  return [
    styles.accent(zh ? "命令面板" : "Command palette"),
    styles.muted(`${zh ? "搜索" : "Search"}: ${view.query ?? ""}▌`),
    ...items.map((item, index) => {
      const prefix = index === selected ? styles.active("›") : " ";
      const content = `${item.command.padEnd(12)} ${item.description}`;
      return `${prefix} ${item.enabled ? content : styles.muted(`${content} · ${zh ? "暂不可用" : "unavailable"}`)}`;
    }),
    styles.muted(zh ? "↑↓ 移动 · Enter 打开 · Esc 关闭" : "↑↓ move · Enter open · Esc close"),
  ];
}

function renderControl(
  frame: AgentTuiFrame,
  tab: ControlPanelTab,
  width: number,
  view: OverlayViewState,
  styles: ReturnType<typeof overlayStyles>,
): string[] {
  const zh = frame.ui.preferences.locale === "zh-CN";
  const tabs = [
    ["model", zh ? "模型" : "Model"],
    ["theme", zh ? "主题" : "Theme"],
    ["language", zh ? "语言" : "Language"],
    ["decision", zh ? "决策" : "Decision"],
  ] as const;
  const lines = [
    styles.accent(zh ? "控制面板" : "Control panel"),
    tabs
      .map(([id, label]) => (id === tab ? styles.active(`[ ${label} ]`) : `  ${label}  `))
      .join("   "),
    ...controlSummary(frame, styles),
  ];
  if (tab === "model") lines.push(...renderModels(frame, view.selection ?? 0, styles));
  if (tab === "decision") lines.push(...renderDecisionModel(frame, width, styles));
  if (tab === "theme") lines.push(...renderThemes(frame, view, styles));
  if (tab === "language") lines.push(...renderLanguages(frame, view.selection ?? 0, styles));
  const instructions =
    width < 80
      ? zh
        ? ["↑↓ 移动 · ←→ 切换分区", "Space 预览 · Enter 应用并记住", "Esc 关闭"]
        : ["↑↓ move · ←→ switch section", "Space preview · Enter apply and remember", "Esc close"]
      : [
          zh
            ? "↑↓ 移动 · ←→ 切换分区 · Space 预览 · Enter 应用并记住 · Esc 关闭"
            : "↑↓ move · ←→ switch section · Space preview · Enter apply and remember · Esc close",
        ];
  lines.push(
    ...(tab === "decision"
      ? [
          zh
            ? "←→ 切换分区 · Esc 关闭（安装会在后台继续）"
            : "←→ switch section · Esc close (installation continues in background)",
        ]
      : instructions
    ).map(styles.muted),
  );
  return lines;
}

function controlSummary(frame: AgentTuiFrame, styles: ReturnType<typeof overlayStyles>): string[] {
  const zh = frame.ui.preferences.locale === "zh-CN";
  const route = frame.snapshot?.route?.selected;
  const preferred = frame.snapshot?.modelCatalog?.models.find((model) => model.selectableForWorker);
  const model =
    route === undefined
      ? preferred === undefined
        ? "auto"
        : `auto · ${preferred.route.provider}/${preferred.route.model}`
      : `${route.provider}/${route.model}`;
  const [themeEnglish, themeChinese] = THEME_NAMES[frame.ui.preferences.themeId];
  const language =
    frame.ui.preferences.locale === "zh-CN"
      ? "简体中文 · English available"
      : "English · 简体中文 available";
  const entries = [
    `${zh ? "模型" : "Model"}: ${safeTerminalText(model)}`,
    `${zh ? "主题" : "Theme"}: ${themeEnglish} · ${themeChinese}`,
    `${zh ? "语言" : "Language"}: ${language}`,
  ];
  return entries.map(styles.secondary);
}

function renderModels(
  frame: AgentTuiFrame,
  selection: number,
  styles: ReturnType<typeof overlayStyles>,
): string[] {
  const zh = frame.ui.preferences.locale === "zh-CN";
  const models = frame.snapshot?.modelCatalog?.models ?? [];
  const entries = [
    { label: zh ? "自动 · 本地优先" : "Auto · local preferred", enabled: true },
    ...models.map((model) => ({
      label: `${model.route.provider}/${model.route.model} · ${model.locality} · ${String(model.contextWindow)} ctx`,
      enabled: model.selectableForWorker,
    })),
  ];
  const selected = clamp(selection, entries.length);
  return [
    styles.muted(
      zh
        ? "模型变更从下一 step 生效；会话上下文保持不变。"
        : "Model changes apply from the next step; session context is preserved.",
    ),
    ...entries.map(
      (entry, index) =>
        `${index === selected ? styles.active("›") : " "} ${entry.enabled ? safeTerminalText(entry.label) : styles.muted(`${safeTerminalText(entry.label)} · disabled`)}`,
    ),
  ];
}

function renderThemes(
  frame: AgentTuiFrame,
  view: OverlayViewState,
  styles: ReturnType<typeof overlayStyles>,
): string[] {
  const zh = frame.ui.preferences.locale === "zh-CN";
  const selected = clamp(
    view.selection ?? THEME_IDS.indexOf(frame.ui.preferences.themeId),
    THEME_IDS.length,
  );
  return THEME_IDS.map((id, index) => {
    const [english, chinese] = THEME_NAMES[id];
    const badges = [
      id === "hearth-teal" ? (zh ? "默认" : "default") : "",
      id === (view.savedTheme ?? frame.ui.preferences.themeId) ? (zh ? "已保存" : "saved") : "",
      id === view.previewTheme ? (zh ? "正在预览" : "previewing") : "",
    ].filter((value) => value.length > 0);
    return `${index === selected ? styles.active("›") : " "} ${english} · ${chinese}${badges.length === 0 ? "" : `  [${badges.join(" · ")}]`}`;
  });
}

function renderLanguages(
  frame: AgentTuiFrame,
  selection: number,
  styles: ReturnType<typeof overlayStyles>,
): string[] {
  const entries = [
    ["en-US", "English"],
    ["zh-CN", "简体中文"],
  ] as const;
  const selected = clamp(selection, entries.length);
  return entries.map(
    ([id, label], index) =>
      `${index === selected ? styles.active("›") : " "} ${label}${frame.ui.preferences.locale === id ? "  [saved]" : ""}`,
  );
}

function overlayStyles(themeId: ThemeId) {
  const theme = THEMES[themeId];
  const color =
    (key: keyof typeof theme, bold = false, background = false) =>
    (value: string) =>
      styleText(
        value,
        background
          ? { fg: parseColor(theme.textPrimary), bg: parseColor(theme[key]), bold }
          : { fg: parseColor(theme[key]), bold },
        "truecolor",
      );
  return {
    accent: color("accentPrimary", true),
    secondary: color("accentSecondary"),
    muted: color("textSecondary"),
    success: color("statusSuccess", true),
    warning: color("statusWarning", true),
    danger: color("statusDanger", true),
    active: color("surfaceActive", true, true),
  };
}

function box(lines: readonly string[], width: number, themeId: ThemeId): string[] {
  const inner = Math.max(1, width - 4);
  const theme = THEMES[themeId];
  const border = theme.borderDefault;
  const paint = (value: string) => styleText(value, { fg: parseColor(border) }, "truecolor");
  const background = backgroundAnsi(parseColor(theme.surfaceRaised), "truecolor");
  return [
    paint(`╭${"─".repeat(width - 2)}╮`),
    ...lines.map((line) => {
      const clipped = truncateToWidth(line, inner, "");
      const padding = " ".repeat(Math.max(0, inner - visibleWidth(clipped)));
      return `${paint("│")} ${clipped}${padding} ${paint("│")}`;
    }),
    paint(`╰${"─".repeat(width - 2)}╯`),
  ].map(
    (line) => `${background}${line.replaceAll("\u001b[49m", `\u001b[49m${background}`)}\u001b[49m`,
  );
}

function clamp(index: number, length: number): number {
  if (length <= 0) return 0;
  return Math.max(0, Math.min(index, length - 1));
}
