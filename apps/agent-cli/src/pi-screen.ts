import {
  CombinedAutocompleteProvider,
  Editor,
  Key,
  ProcessTerminal,
  TuiMainScreen,
  matchesKey,
  parseColor,
  styleText,
  type Component,
  type EditorTheme,
  type Terminal,
  type TUI,
} from "@earendil-works/pi-tui";

import type { AgentTuiFrame } from "./controller.ts";
import { renderFrame } from "./surface.ts";
import { THEMES } from "./themes.ts";

export interface PiScreenOptions {
  readonly terminal?: Terminal;
  readonly frame: AgentTuiFrame;
  readonly onSubmit: (text: string) => void;
  readonly onLanguageToggle: () => void;
  readonly onPaletteToggle: () => void;
  readonly onActivityToggle: () => void;
  readonly onExit: () => void;
}

export interface PiScreen {
  start(): void;
  stop(): void;
  update(frame: AgentTuiFrame): void;
  editorText(): string;
}

export function createPiScreen(options: PiScreenOptions): PiScreen {
  const tui: TUI = new TuiMainScreen(options.terminal ?? new ProcessTerminal());
  const surface = new FrameComponent(options.frame);
  const editor = new Editor(tui, editorTheme(options.frame), { paddingX: 1 });
  editor.setAutocompleteProvider(new CombinedAutocompleteProvider([...COMMANDS], process.cwd()));
  editor.onSubmit = (text) => {
    const value = text.trim();
    if (value.length === 0) return;
    editor.addToHistory(value);
    editor.setText("");
    options.onSubmit(value);
  };
  tui.addChild(surface);
  tui.addChild(editor);
  tui.setFocus(editor);
  tui.addInputListener((data) => {
    if (matchesKey(data, Key.f2)) {
      options.onLanguageToggle();
      return { consume: true };
    }
    if (matchesKey(data, Key.ctrl("p"))) {
      options.onPaletteToggle();
      return { consume: true };
    }
    if (matchesKey(data, Key.ctrlShift("a"))) {
      options.onActivityToggle();
      return { consume: true };
    }
    if (matchesKey(data, Key.ctrl("c"))) {
      options.onExit();
      return { consume: true };
    }
    return undefined;
  });

  return {
    start: () => {
      tui.start();
    },
    stop: () => {
      tui.stop();
    },
    update: (frame) => {
      surface.update(frame);
      editor.borderColor = colorizer(frame.ui.preferences.themeId, "accent");
      editor.invalidate();
      tui.requestRender();
    },
    editorText: () => editor.getExpandedText(),
  };
}

class FrameComponent implements Component {
  public constructor(private frame: AgentTuiFrame) {}

  public update(frame: AgentTuiFrame): void {
    this.frame = frame;
  }

  public invalidate(): void {
    // The component keeps no derived render cache.
  }

  public render(width: number): string[] {
    return renderFrame(this.frame, width);
  }
}

function editorTheme(frame: AgentTuiFrame): EditorTheme {
  const themeId = frame.ui.preferences.themeId;
  return {
    borderColor: colorizer(themeId, "accent"),
    selectList: {
      selectedPrefix: colorizer(themeId, "accent"),
      selectedText: colorizer(themeId, "accent", true),
      description: colorizer(themeId, "muted"),
      scrollInfo: colorizer(themeId, "muted"),
      noMatch: colorizer(themeId, "warning"),
    },
  };
}

function colorizer(
  themeId: keyof typeof THEMES,
  role: keyof (typeof THEMES)[keyof typeof THEMES],
  bold = false,
): (value: string) => string {
  return (value) => styleText(value, { fg: parseColor(THEMES[themeId][role]), bold }, "truecolor");
}

const COMMANDS = [
  { name: "language", description: "Switch English / 中文", argumentHint: "en|zh" },
  { name: "theme", description: "Choose one of six themes" },
  { name: "download", description: "Download the selected local model" },
  { name: "retry", description: "Retry a failed model download" },
  { name: "cancel", description: "Cancel the active download or turn" },
  { name: "steer", description: "Steer the active turn" },
  { name: "follow-up", description: "Queue a follow-up" },
  { name: "approve", description: "Approve a pending tool" },
  { name: "deny", description: "Deny a pending tool" },
  { name: "activity", description: "Toggle Activity Inspector" },
  { name: "commands", description: "Toggle command palette" },
] as const;
