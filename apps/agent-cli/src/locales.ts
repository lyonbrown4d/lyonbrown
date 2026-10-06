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
  readonly terminalTooSmall: string;
  readonly modelSetup: {
    readonly phase: Record<
      | "required"
      | "waiting-for-lock"
      | "checking"
      | "downloading"
      | "verifying"
      | "installing"
      | "activating"
      | "ready"
      | "cancelled"
      | "failed",
      string
    >;
    readonly recommended: string;
    readonly installedPreferred: string;
    readonly hardwareCompatible: string;
    readonly conservativeFallback: string;
    readonly confirmDownload: string;
    readonly download: string;
    readonly retry: string;
    readonly cancel: string;
    readonly exit: string;
    readonly actionsDisabled: string;
  };
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
  terminalTooSmall: "Terminal too small · resize to at least 30 columns",
  modelSetup: {
    phase: {
      required: "Required",
      "waiting-for-lock": "Waiting for another download",
      checking: "Checking",
      downloading: "Downloading",
      verifying: "Verifying",
      installing: "Installing",
      activating: "Activating",
      ready: "Ready",
      cancelled: "Cancelled",
      failed: "Failed",
    },
    recommended: "Recommended model",
    installedPreferred: "Installed model",
    hardwareCompatible: "Recommended for this hardware",
    conservativeFallback: "Conservative compatibility choice",
    confirmDownload: "download only after confirmation",
    download: "Download",
    retry: "Retry",
    cancel: "Cancel",
    exit: "Exit",
    actionsDisabled: "Actions disabled until the daemon reconnects",
  },
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
  terminalTooSmall: "终端过小 · 请调整到至少 30 列",
  modelSetup: {
    phase: {
      required: "需要模型",
      "waiting-for-lock": "正在等待其他下载",
      checking: "正在检查",
      downloading: "正在下载",
      verifying: "正在校验",
      installing: "正在安装",
      activating: "正在激活",
      ready: "已就绪",
      cancelled: "已取消",
      failed: "失败",
    },
    recommended: "推荐模型",
    installedPreferred: "已安装模型",
    hardwareCompatible: "根据当前硬件推荐",
    conservativeFallback: "保守兼容选择",
    confirmDownload: "仅在确认后下载",
    download: "下载",
    retry: "重试",
    cancel: "取消",
    exit: "退出",
    actionsDisabled: "Daemon 重连前操作已禁用",
  },
};

export function labels(locale: Locale): Labels {
  return locale === "zh-CN" ? ZH : EN;
}
