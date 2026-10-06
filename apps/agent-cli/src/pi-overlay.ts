import { Key, matchesKey, type Component, type TUI } from "@earendil-works/pi-tui";

import { commandCatalog } from "@lyonbrown/agent-cli/internal/command-catalog";
import type { AgentTuiFrame } from "@lyonbrown/agent-cli/internal/controller";
import { renderOverlay } from "@lyonbrown/agent-cli/internal/overlay-surface";
import { THEMES } from "@lyonbrown/agent-cli/internal/themes";

export interface FrameOverlay extends Component {
  readonly kind: NonNullable<AgentTuiFrame["overlay"]>["kind"];
  update(frame: AgentTuiFrame): void;
}

export function createFrameOverlay(
  frame: AgentTuiFrame,
  tui: TUI,
  submit: (command: string) => unknown,
  close: () => void,
  previewThemeChange: (themeId: keyof typeof THEMES) => void,
): FrameOverlay {
  return new FrameOverlayComponent(frame, tui, submit, close, previewThemeChange);
}

class FrameOverlayComponent implements FrameOverlay {
  public readonly kind: NonNullable<AgentTuiFrame["overlay"]>["kind"];
  private frame: AgentTuiFrame;
  private selection = 0;
  private query = "";
  private tab: "model" | "theme" | "language";
  private previewTheme: keyof typeof THEMES | null = null;
  private readonly savedTheme: keyof typeof THEMES;
  private activitySelection: { readonly turnId: string; readonly stepId: string } | null = null;
  private activityFollowLatest = true;
  private activityPane: "list" | "detail" = "list";
  private activityDetailOffset = 0;

  public constructor(
    frame: AgentTuiFrame,
    private readonly tui: TUI,
    private readonly submit: (command: string) => unknown,
    private readonly close: () => void,
    private readonly previewThemeChange: (themeId: keyof typeof THEMES) => void,
  ) {
    const overlay = frame.overlay;
    if (overlay === null) throw new Error("overlay frame is required");
    this.frame = frame;
    this.kind = overlay.kind;
    this.tab = overlay.kind === "control" ? overlay.tab : "model";
    this.savedTheme = frame.ui.preferences.themeId;
    this.selection = this.defaultControlSelection(this.tab);
  }

  public update(frame: AgentTuiFrame): void {
    this.frame = frame;
  }

  public invalidate(): void {
    // Rendering is derived from the latest immutable frame; no cache is retained.
  }

  public render(width: number): string[] {
    return renderOverlay(this.frame, width, {
      selection: this.selection,
      query: this.query,
      controlTab: this.tab,
      previewTheme: this.previewTheme,
      savedTheme: this.savedTheme,
      activitySelection: this.resolveActivityIndex(),
      activityPane: this.activityPane,
      activityDetailOffset: this.activityDetailOffset,
    });
  }

  public handleInput(data: string): void {
    if (matchesKey(data, Key.escape)) {
      if (this.previewTheme !== null && this.frame.ui.preferences.themeId !== this.savedTheme) {
        this.previewThemeChange(this.savedTheme);
      }
      this.close();
      return;
    }
    if (this.kind === "activity" && this.handleActivityInput(data)) {
      this.tui.requestRender();
      return;
    }
    if (matchesKey(data, Key.up)) this.selection = Math.max(0, this.selection - 1);
    else if (matchesKey(data, Key.down)) this.selection += 1;
    else if (this.kind === "control" && matchesKey(data, Key.left)) this.switchTab(-1);
    else if (this.kind === "control" && matchesKey(data, Key.right)) this.switchTab(1);
    else if (this.kind === "control" && matchesKey(data, Key.space)) this.previewSelection();
    else if (matchesKey(data, Key.enter)) this.activateSelection();
    else if (this.kind === "commands" && matchesKey(data, Key.backspace)) {
      this.query = this.query.slice(0, -1);
      this.selection = 0;
    } else if (this.kind === "commands" && data.length === 1 && data >= " ") {
      this.query += data;
      this.selection = 0;
    }
    this.tui.requestRender();
  }

  private switchTab(delta: number): void {
    const tabs = ["model", "theme", "language"] as const;
    const current = tabs.indexOf(this.tab);
    this.tab = tabs[(current + delta + tabs.length) % tabs.length] ?? "model";
    this.selection = this.defaultControlSelection(this.tab);
  }

  private handleActivityInput(data: string): boolean {
    if (matchesKey(data, Key.up) || matchesKey(data, Key.down)) {
      const items = this.activityItems();
      if (items.length === 0) return true;
      const current = this.resolveActivityIndex();
      const next = Math.max(
        0,
        Math.min(items.length - 1, current + (matchesKey(data, Key.down) ? 1 : -1)),
      );
      const item = items[next];
      this.activityFollowLatest = next === items.length - 1;
      this.activitySelection = this.activityFollowLatest || item === undefined ? null : item;
      this.activityDetailOffset = 0;
      return true;
    }
    if (matchesKey(data, Key.enter) || matchesKey(data, Key.right) || matchesKey(data, Key.tab)) {
      this.activityPane = "detail";
      return true;
    }
    if (matchesKey(data, Key.left) || matchesKey(data, Key.shift("tab"))) {
      this.activityPane = "list";
      return true;
    }
    if (matchesKey(data, Key.pageDown)) {
      this.activityDetailOffset += 6;
      return true;
    }
    if (matchesKey(data, Key.pageUp)) {
      this.activityDetailOffset = Math.max(0, this.activityDetailOffset - 6);
      return true;
    }
    if (matchesKey(data, Key.home)) {
      this.activityDetailOffset = 0;
      return true;
    }
    if (matchesKey(data, Key.end)) {
      this.activityDetailOffset = Number.MAX_SAFE_INTEGER;
      return true;
    }
    return false;
  }

  private activityItems(): readonly { readonly turnId: string; readonly stepId: string }[] {
    return (
      this.frame.snapshot?.turns.flatMap((turn) =>
        turn.activities.map((activity) => ({ turnId: turn.turnId, stepId: activity.stepId })),
      ) ?? []
    );
  }

  private resolveActivityIndex(): number {
    const items = this.activityItems();
    if (items.length === 0) return 0;
    if (this.activityFollowLatest || this.activitySelection === null) return items.length - 1;
    const selection = this.activitySelection;
    const index = items.findIndex(
      (item) => item.turnId === selection.turnId && item.stepId === selection.stepId,
    );
    if (index >= 0) return index;
    this.activityFollowLatest = true;
    this.activitySelection = null;
    this.activityDetailOffset = 0;
    return items.length - 1;
  }

  private previewSelection(): void {
    if (this.tab !== "theme") return;
    const ids = Object.keys(THEMES) as readonly (keyof typeof THEMES)[];
    const id = ids[Math.max(0, Math.min(this.selection, ids.length - 1))];
    if (id === undefined) return;
    this.previewTheme = id;
    this.previewThemeChange(id);
  }

  private activateSelection(): void {
    if (this.kind === "commands") {
      const item = this.selectedCommand();
      if (item?.enabled === true) void Promise.resolve(this.submit(item.command));
      return;
    }
    if (this.kind !== "control") return;
    if (this.tab === "theme") {
      this.previewSelection();
      if (this.previewTheme !== null)
        void Promise.resolve(this.submit(`/theme ${this.previewTheme}`));
      this.close();
      return;
    }
    if (this.tab === "language") {
      void Promise.resolve(
        this.submit(this.selection === 0 ? "/language en-US" : "/language zh-CN"),
      );
      this.close();
      return;
    }
    const models =
      this.frame.snapshot?.modelCatalog?.models.filter((model) => model.selectableForWorker) ?? [];
    const selected = this.selection === 0 ? null : models[this.selection - 1];
    void Promise.resolve(
      this.submit(
        selected === null || selected === undefined
          ? "/model auto"
          : `/model ${selected.route.provider}/${selected.route.model}`,
      ),
    );
    this.close();
  }

  private selectedCommand() {
    const query = this.query.toLocaleLowerCase();
    const items = commandCatalog(this.frame.ui.preferences.locale).filter(
      (item) =>
        query.length === 0 ||
        `${item.command} ${item.description}`.toLocaleLowerCase().includes(query),
    );
    return items[Math.max(0, Math.min(this.selection, items.length - 1))] ?? null;
  }

  private defaultControlSelection(tab: "model" | "theme" | "language"): number {
    if (tab === "theme") {
      return Math.max(0, Object.keys(THEMES).indexOf(this.frame.ui.preferences.themeId));
    }
    if (tab === "language") return this.frame.ui.preferences.locale === "en-US" ? 0 : 1;
    const active = this.frame.snapshot?.modelPolicy?.active;
    if (active === undefined || active.kind === "auto") return 0;
    const models =
      this.frame.snapshot?.modelCatalog?.models.filter((model) => model.selectableForWorker) ?? [];
    const index = models.findIndex(
      (model) => model.route.provider === active.provider && model.route.model === active.model,
    );
    return index < 0 ? 0 : index + 1;
  }
}
