import type { ActivitySnapshotDto, RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import type { Locale } from "@lyonbrown/ui-model";
import { visibleWidth } from "@earendil-works/pi-tui";

import type { SurfaceStyles } from "@lyonbrown/agent-cli/internal/surface-types";
import { safeTerminalText } from "@lyonbrown/agent-cli/internal/text-safety";

export interface ActivityInspectorView {
  readonly width: number;
  readonly selection: number;
  readonly narrowPane: "list" | "detail";
  readonly detailOffset: number;
  readonly detailLimit?: number;
}

export function renderActivityInspector(
  snapshot: RuntimeSnapshotDto | null,
  locale: Locale,
  fresh: boolean,
  styles: SurfaceStyles,
  view: ActivityInspectorView = {
    width: 80,
    selection: Number.MAX_SAFE_INTEGER,
    narrowPane: "list",
    detailOffset: 0,
  },
): string[] {
  const zh = locale === "zh-CN";
  const activities =
    snapshot?.turns.flatMap((turn) =>
      turn.activities.map((activity) => ({ turnId: turn.turnId, activity })),
    ) ?? [];
  const header = styles.accent(
    `${zh ? "活动检查器" : "Activity Inspector"} · ${fresh ? (zh ? "状态已同步" : "synced") : zh ? "过期缓存" : "stale cache"}`,
  );
  if (activities.length === 0) return [header, styles.muted(zh ? "暂无活动" : "No activity")];
  const selectedIndex = Math.max(0, Math.min(view.selection, activities.length - 1));
  const list = activities.slice(-8).map((item) => {
    const absoluteIndex = activities.indexOf(item);
    const marker = absoluteIndex === selectedIndex ? styles.active("▸") : " ";
    return `${marker} ${safeTerminalText(item.activity.stepId)} · ${safeTerminalText(item.activity.phase)} · ${String(item.activity.tools.length)} ${zh ? "个工具" : "tools"}`;
  });
  const selected = activities[selectedIndex];
  if (selected === undefined) return [header, ...list];
  const evidence = renderEvidence(selected.turnId, selected.activity, locale, styles);
  const detailLimit = view.detailLimit ?? 12;
  const detailOffset = Math.min(
    Math.max(0, view.detailOffset),
    Math.max(0, evidence.length - detailLimit),
  );
  const detail = evidence.slice(detailOffset, detailOffset + detailLimit);
  const help = styles.muted(
    zh ? "↑↓ 选择 · PgUp/PgDn 滚动 · Ctrl-O 关闭" : "↑↓ select · PgUp/PgDn scroll · Ctrl-O close",
  );
  if (view.width >= 90) {
    return [header, ...columns(list, detail, Math.max(24, Math.floor(view.width * 0.35))), help];
  }
  if (view.width >= 52) {
    return [header, ...list, styles.muted("─".repeat(Math.min(40, view.width))), ...detail, help];
  }
  return view.narrowPane === "detail"
    ? [header, ...detail, styles.muted(zh ? "← 返回列表" : "← back to list"), help]
    : [header, ...list, styles.muted(zh ? "Enter/→ 查看详情" : "Enter/→ view details"), help];
}

function columns(left: readonly string[], right: readonly string[], leftWidth: number): string[] {
  const length = Math.max(left.length, right.length);
  return Array.from({ length }, (_, index) => {
    const leftLine = left[index] ?? "";
    const rightLine = right[index] ?? "";
    return `${padVisible(leftLine, leftWidth)} │ ${rightLine}`;
  });
}

function padVisible(value: string, width: number): string {
  return `${value}${" ".repeat(Math.max(0, width - visibleWidth(value)))}`;
}

function renderEvidence(
  turnId: string,
  activity: ActivitySnapshotDto,
  locale: Locale,
  styles: SurfaceStyles,
): string[] {
  const zh = locale === "zh-CN";
  const route = activity.route;
  const usage = activity.usage;
  const context = activity.context;
  const lines = [
    `${zh ? "定位" : "Identity"}  ${safeTerminalText(turnId)} / ${safeTerminalText(activity.stepId)}`,
  ];
  if (route !== null) {
    lines.push(
      `${zh ? "路由" : "Route"}     ${safeTerminalText(route.selected.provider)}/${safeTerminalText(route.selected.model)} · ${safeTerminalText(route.reason)}`,
    );
  }
  if (context !== null) {
    lines.push(
      `${zh ? "上下文" : "Context"}   ${String(context.systemTokens)} system · ${String(context.contextTokens)} context · ${String(context.dropped)} dropped`,
    );
  }
  if (usage !== null) {
    lines.push(
      `${zh ? "用量" : "Usage"}     ${String(usage.input)} input · ${String(usage.output)} output · ${String(usage.cacheRead)} cache`,
    );
  }
  if (activity.assistantText.length > 0) {
    lines.push(`${zh ? "输出" : "Assistant"}  ${safeTerminalText(activity.assistantText)}`);
  }
  for (const tool of activity.tools.slice(0, 4)) {
    lines.push(
      styles.warning(
        `${zh ? "工具" : "Tool"}      ${safeTerminalText(tool.name)} · ${safeTerminalText(tool.state)}`,
      ),
      `          ${truncate(safeTerminalText(tool.argumentsJson), 240)}`,
    );
    if (tool.result !== null) {
      lines.push(
        `          ${tool.result.ok ? "✓" : "!"} ${truncate(safeTerminalText(tool.result.output.length > 0 ? tool.result.output : (tool.result.error ?? "—")), 320)} · ${String(tool.result.durationMs)} ms`,
      );
    }
  }
  return lines;
}

function truncate(value: string, limit: number): string {
  return value.length <= limit ? value : `${value.slice(0, limit - 1)}…`;
}
