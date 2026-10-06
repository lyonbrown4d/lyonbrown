import type { Locale } from "@lyonbrown/ui-model";

export interface CommandCatalogItem {
  readonly command: string;
  readonly description: string;
  readonly enabled: boolean;
}

const COMMANDS = [
  "model",
  "theme",
  "language",
  "sessions",
  "new",
  "resume",
  "status",
  "queue",
  "help",
  "quit",
] as const;

const DESCRIPTIONS: Readonly<Record<Locale, readonly string[]>> = {
  "en-US": [
    "Model and routing policy",
    "Preview and save a theme",
    "English / 简体中文",
    "Select or resume a session",
    "Start a new session",
    "Resume a session",
    "Runtime and route status",
    "Queue a follow-up message",
    "Commands and shortcuts",
    "Exit the client",
  ],
  "zh-CN": [
    "模型与路由策略",
    "预览并保存主题",
    "English / 简体中文",
    "选择或恢复会话",
    "新建会话",
    "恢复会话",
    "Runtime 与路由状态",
    "排队下一步消息",
    "命令与快捷键",
    "退出客户端",
  ],
};

const ENABLED = new Set(["model", "theme", "language", "status", "help", "quit"]);

export function commandCatalog(locale: Locale): readonly CommandCatalogItem[] {
  return COMMANDS.map((name, index) => ({
    command: `/${name}`,
    description: DESCRIPTIONS[locale][index] ?? name,
    enabled: ENABLED.has(name),
  }));
}
