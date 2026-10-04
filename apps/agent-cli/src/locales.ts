import type { Locale } from "@lyonbrown/ui-model";

export interface Labels {
  readonly connection: Record<
    "disconnected" | "connecting" | "synchronizing" | "ready" | "stale" | "unavailable",
    string
  >;
  readonly user: string;
  readonly assistant: string;
  readonly model: string;
  readonly approval: string;
  readonly commands: string;
  readonly activity: string;
  readonly empty: string;
  readonly footer: string;
}

const EN: Labels = {
  connection: {
    disconnected: "Disconnected",
    connecting: "Connecting",
    synchronizing: "Synchronizing",
    ready: "Ready",
    stale: "Stale — commands disabled",
    unavailable: "Daemon unavailable",
  },
  user: "You",
  assistant: "Agent",
  model: "Local model",
  approval: "Approval required",
  commands: "Command palette",
  activity: "Activity Inspector",
  empty: "Start typing while Lyonbrown connects in the background.",
  footer: "F2 language · Ctrl+P commands · Ctrl+Shift+A activity · Ctrl+C exit",
};

const ZH: Labels = {
  connection: {
    disconnected: "未连接",
    connecting: "正在连接",
    synchronizing: "正在同步",
    ready: "已就绪",
    stale: "连接已过期 — 命令已禁用",
    unavailable: "Daemon 不可用",
  },
  user: "你",
  assistant: "Agent",
  model: "本地模型",
  approval: "需要批准",
  commands: "命令面板",
  activity: "活动检查器",
  empty: "可以立即输入；Lyonbrown 会在后台连接。",
  footer: "F2 语言 · Ctrl+P 命令 · Ctrl+Shift+A 活动 · Ctrl+C 退出",
};

export function labels(locale: Locale): Labels {
  return locale === "zh-CN" ? ZH : EN;
}
