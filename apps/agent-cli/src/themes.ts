import type { ThemeId } from "@lyonbrown/ui-model";

export interface ThemePalette {
  readonly accent: string;
  readonly secondary: string;
  readonly muted: string;
  readonly success: string;
  readonly warning: string;
  readonly danger: string;
}

export const THEMES: Readonly<Record<ThemeId, ThemePalette>> = {
  "carbon-mint": {
    accent: "#7bdcb5",
    secondary: "#a5b4fc",
    muted: "#8b949e",
    success: "#56d364",
    warning: "#e3b341",
    danger: "#ff7b72",
  },
  "ink-violet": {
    accent: "#c4b5fd",
    secondary: "#93c5fd",
    muted: "#9ca3af",
    success: "#86efac",
    warning: "#fcd34d",
    danger: "#fda4af",
  },
  "slate-blue": {
    accent: "#7dd3fc",
    secondary: "#818cf8",
    muted: "#94a3b8",
    success: "#6ee7b7",
    warning: "#fde68a",
    danger: "#fca5a5",
  },
  "hearth-teal": {
    accent: "#d97757",
    secondary: "#5fb3a6",
    muted: "#9b948d",
    success: "#78b159",
    warning: "#d9a441",
    danger: "#d66a6a",
  },
  "sandstone-espresso": {
    accent: "#f0b27a",
    secondary: "#d4a373",
    muted: "#a89984",
    success: "#a3b18a",
    warning: "#e9c46a",
    danger: "#e76f51",
  },
  "graphite-coral": {
    accent: "#ff8a80",
    secondary: "#80cbc4",
    muted: "#9e9e9e",
    success: "#81c784",
    warning: "#ffd54f",
    danger: "#ef5350",
  },
};
