import type { AgentTuiFrame } from "@lyonbrown/agent-cli/internal/controller";
import type { SurfaceStyles } from "@lyonbrown/agent-cli/internal/surface-types";
import { safeTerminalText } from "@lyonbrown/agent-cli/internal/text-safety";

export function renderSessionsOverlay(frame: AgentTuiFrame, styles: SurfaceStyles): string[] {
  const zh = frame.ui.preferences.locale === "zh-CN";
  return [
    styles.accent(zh ? "会话" : "Sessions"),
    styles.warning(
      zh ? "当前 daemon 尚未提供会话目录接口" : "The daemon does not expose a session catalog yet",
    ),
    styles.muted(zh ? "Esc  关闭" : "Esc  close"),
  ];
}

export function renderStatusOverlay(frame: AgentTuiFrame, styles: SurfaceStyles): string[] {
  const zh = frame.ui.preferences.locale === "zh-CN";
  const snapshot = frame.snapshot;
  const selected = snapshot?.route?.selected;
  return [
    styles.accent(zh ? "运行状态" : "Runtime status"),
    `${zh ? "连接" : "Connection"}  ${frame.ui.remote.connection}${frame.ui.remote.snapshotFresh ? (zh ? " · 最新" : " · fresh") : zh ? " · 已过期" : " · stale"}`,
    `${zh ? "会话" : "Session"}     ${safeTerminalText(snapshot?.sessionId ?? "—")}`,
    `${zh ? "阶段" : "Phase"}       ${safeTerminalText(snapshot?.phase ?? "—")}`,
    `${zh ? "路由" : "Route"}       ${selected === undefined ? "auto" : `${safeTerminalText(selected.provider)}/${safeTerminalText(selected.model)}`}`,
    `${zh ? "版本" : "Revision"}    ${String(snapshot?.revision ?? 0n)}`,
    styles.muted(zh ? "Esc  关闭" : "Esc  close"),
  ];
}

export function renderHelpOverlay(frame: AgentTuiFrame, styles: SurfaceStyles): string[] {
  const zh = frame.ui.preferences.locale === "zh-CN";
  return [
    styles.accent(zh ? "帮助" : "Help"),
    "Enter       " + (zh ? "提交 / 立即引导" : "submit / steer now"),
    "Alt-Enter   " + (zh ? "排队到下一步" : "queue for next step"),
    "Ctrl-C      " + (zh ? "取消；再次按下强制停止" : "cancel; press again to force stop"),
    "Ctrl-O      " + (zh ? "活动详情" : "activity details"),
    "Ctrl-P      " + (zh ? "命令面板" : "command palette"),
    "F2          " + (zh ? "切换语言" : "switch language"),
    styles.muted(zh ? "Esc  关闭" : "Esc  close"),
  ];
}
