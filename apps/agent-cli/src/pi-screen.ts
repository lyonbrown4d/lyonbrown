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
  type OverlayHandle,
} from "@earendil-works/pi-tui";

import type { AgentTuiFrame } from "@lyonbrown/agent-cli/internal/controller";
import { commandCatalog } from "@lyonbrown/agent-cli/internal/command-catalog";
import { createFrameOverlay, type FrameOverlay } from "@lyonbrown/agent-cli/internal/pi-overlay";
import { setupKeyIntent } from "@lyonbrown/agent-cli/internal/setup-input";
import { renderFrame } from "@lyonbrown/agent-cli/internal/surface";
import { THEMES } from "@lyonbrown/agent-cli/internal/themes";

export interface PiScreenOptions {
  readonly terminal?: Terminal;
  readonly frame: AgentTuiFrame;
  readonly onSubmit: (text: string) => unknown;
  readonly onLanguageToggle: () => void;
  readonly onPaletteToggle: () => void;
  readonly onActivityToggle: () => void;
  readonly onExit: () => void;
  readonly onOverlayClose?: () => void;
  readonly onThemePreview?: (themeId: keyof typeof THEMES) => void;
}

export interface PiScreen {
  start(): void;
  stop(): void;
  update(frame: AgentTuiFrame): void;
  editorText(): string;
}

export function createPiScreen(options: PiScreenOptions): PiScreen {
  const tui: TUI = new TuiMainScreen(options.terminal ?? new ProcessTerminal());
  const surface = new FrameComponent(options.frame, () => tui.terminal.rows);
  let currentFrame = options.frame;
  let setupCommandPending: Promise<boolean> | null = null;
  let exitAfterSetupCommand = false;
  let overlayHandle: OverlayHandle | null = null;
  let overlayComponent: FrameOverlay | null = null;
  let autocompleteLocale = options.frame.ui.preferences.locale;
  let editorThemeId = options.frame.ui.preferences.themeId;
  let approvalSelection = 0;
  let approvalRequestId: string | null = options.frame.snapshot?.pendingApproval?.requestId ?? null;
  const editor = new Editor(
    tui,
    editorTheme(() => editorThemeId),
    { paddingX: 1 },
  );
  editor.setAutocompleteProvider(
    new CombinedAutocompleteProvider(autocompleteCommands(options.frame), process.cwd()),
  );
  editor.onSubmit = (text) => {
    const value = text.trim();
    if (value.length === 0) return;
    editor.addToHistory(value);
    editor.setText("");
    void Promise.resolve(options.onSubmit(value)).catch(() => undefined);
  };
  tui.addChild(surface);
  tui.addChild(editor);
  tui.setFocus(editor);
  tui.addInputListener((data) => {
    if (matchesKey(data, Key.f2)) {
      options.onLanguageToggle();
      return { consume: true };
    }
    if (currentFrame.overlay !== null) return undefined;
    if (matchesKey(data, Key.ctrl("p"))) {
      options.onPaletteToggle();
      return { consume: true };
    }
    const setupIntent = setupKeyIntent(data, currentFrame);
    if (setupIntent !== null) {
      if (setupIntent === "exit") {
        if (setupCommandPending === null) options.onExit();
        else exitAfterSetupCommand = true;
      } else if (setupIntent !== "blocked" && setupCommandPending === null) {
        const pending = Promise.resolve(options.onSubmit(`/${setupIntent}`)).then(
          (result) => result !== false,
          () => false,
        );
        setupCommandPending = pending;
        void pending.then((succeeded) => {
          if (setupCommandPending !== pending) return;
          setupCommandPending = null;
          const shouldExit = exitAfterSetupCommand;
          exitAfterSetupCommand = false;
          if (shouldExit && succeeded) options.onExit();
        });
      }
      return { consume: true };
    }
    const approval = currentFrame.snapshot?.pendingApproval;
    if (approval !== null && approval !== undefined) {
      if (matchesKey(data, Key.ctrl("c"))) {
        void Promise.resolve(options.onSubmit("/deny once"))
          .catch(() => undefined)
          .then(() => options.onSubmit("/cancel"))
          .catch(() => undefined);
        return { consume: true };
      }
      if (matchesKey(data, Key.up) || matchesKey(data, Key.down)) {
        approvalSelection = Math.max(
          0,
          Math.min(3, approvalSelection + (matchesKey(data, Key.down) ? 1 : -1)),
        );
        surface.setApprovalSelection(approvalSelection);
        tui.requestRender();
        return { consume: true };
      }
      const command = approvalCommand(data, approvalSelection);
      if (command !== null) {
        void Promise.resolve(options.onSubmit(command)).catch(() => undefined);
        return { consume: true };
      }
    }
    if (matchesKey(data, Key.alt("enter"))) {
      const value = editor.getExpandedText().trim();
      if (value.length > 0) {
        editor.setText("");
        void Promise.resolve(options.onSubmit(`/queue ${value}`)).catch(() => undefined);
      }
      return { consume: true };
    }
    if (matchesKey(data, "?") && editor.getExpandedText().length === 0) {
      void Promise.resolve(options.onSubmit("/help")).catch(() => undefined);
      return { consume: true };
    }
    if (matchesKey(data, Key.ctrl("o")) || matchesKey(data, Key.ctrlShift("a"))) {
      options.onActivityToggle();
      return { consume: true };
    }
    if (matchesKey(data, Key.ctrl("c"))) {
      const phase = currentFrame.snapshot?.phase;
      if (phase === "cancelling") {
        void Promise.resolve(options.onSubmit("/force-stop"))
          .catch(() => undefined)
          .finally(options.onExit);
        return { consume: true };
      }
      if (isActiveTurnPhase(phase)) {
        void Promise.resolve(options.onSubmit("/cancel")).catch(() => undefined);
        return { consume: true };
      }
      if (editor.getExpandedText().length > 0) {
        editor.setText("");
        return { consume: true };
      }
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
      currentFrame = frame;
      const nextApprovalId = frame.snapshot?.pendingApproval?.requestId ?? null;
      if (nextApprovalId !== approvalRequestId) {
        approvalRequestId = nextApprovalId;
        approvalSelection = 0;
        surface.setApprovalSelection(0);
      }
      surface.update(frame);
      const nextOverlay = frame.overlay;
      if (nextOverlay === null) {
        overlayHandle?.hide();
        overlayHandle = null;
        overlayComponent = null;
      } else if (overlayComponent === null || overlayComponent.kind !== nextOverlay.kind) {
        overlayHandle?.hide();
        overlayComponent = createFrameOverlay(
          frame,
          tui,
          (command) => options.onSubmit(command),
          () => options.onOverlayClose?.(),
          (themeId) => options.onThemePreview?.(themeId),
        );
        overlayHandle = tui.showOverlay(overlayComponent, {
          width: nextOverlay.kind === "activity" ? "88%" : "74%",
          maxHeight: "80%",
          anchor: "center",
          margin: 1,
        });
      } else {
        overlayComponent.update(frame);
      }
      editor.borderColor = colorizer(frame.ui.preferences.themeId, "focusRing");
      editorThemeId = frame.ui.preferences.themeId;
      if (frame.ui.preferences.locale !== autocompleteLocale) {
        autocompleteLocale = frame.ui.preferences.locale;
        editor.setAutocompleteProvider(
          new CombinedAutocompleteProvider(autocompleteCommands(frame), process.cwd()),
        );
      }
      editor.invalidate();
      tui.requestRender();
    },
    editorText: () => editor.getExpandedText(),
  };
}

function isActiveTurnPhase(
  phase: NonNullable<AgentTuiFrame["snapshot"]>["phase"] | undefined,
): boolean {
  return phase === "preparing" || phase === "streaming" || phase === "running-tool";
}

function approvalCommand(data: string, selection: number): string | null {
  if (matchesKey(data, Key.enter)) {
    return (
      ["/deny once", "/approve once", "/approve session", "/deny remaining"][selection] ??
      "/deny once"
    );
  }
  if (matchesKey(data, "d") || matchesKey(data, Key.escape)) return "/deny once";
  if (matchesKey(data, "a")) return "/approve once";
  if (matchesKey(data, "s")) return "/approve session";
  if (matchesKey(data, "x")) return "/deny remaining";
  return null;
}

class FrameComponent implements Component {
  private approvalSelection = 0;

  public constructor(
    private frame: AgentTuiFrame,
    private readonly height: () => number,
  ) {}

  public update(frame: AgentTuiFrame): void {
    this.frame = frame;
  }

  public invalidate(): void {
    // The component keeps no derived render cache.
  }

  public setApprovalSelection(selection: number): void {
    this.approvalSelection = selection;
  }

  public render(width: number): string[] {
    return renderFrame(this.frame, width, {
      approvalSelection: this.approvalSelection,
      height: this.height(),
    });
  }
}

function editorTheme(themeId: () => keyof typeof THEMES): EditorTheme {
  return {
    borderColor: (value) => colorizer(themeId(), "focusRing")(value),
    selectList: {
      selectedPrefix: (value) => colorizer(themeId(), "accentPrimary")(value),
      selectedText: (value) => colorizer(themeId(), "accentPrimary", true)(value),
      description: (value) => colorizer(themeId(), "textSecondary")(value),
      scrollInfo: (value) => colorizer(themeId(), "textFaint")(value),
      noMatch: (value) => colorizer(themeId(), "statusWarning")(value),
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

function autocompleteCommands(frame: AgentTuiFrame) {
  return commandCatalog(frame.ui.preferences.locale).map((item) => ({
    name: item.command.slice(1),
    description: item.description,
    ...(item.command === "/queue" ? { argumentHint: "message" } : {}),
  }));
}
