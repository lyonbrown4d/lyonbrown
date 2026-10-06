import type { RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import type { AgentLauncher } from "@lyonbrown/agent-launch";
import {
  createInitialUiModelState,
  type PreferencesState,
  type UiModelState,
} from "@lyonbrown/ui-model";

import type { ClientPreferenceStore } from "@lyonbrown/agent-cli/internal/client-preferences";
import type { ControlPanelTab } from "@lyonbrown/agent-cli/internal/commands";

export interface AgentTuiFrame {
  readonly ui: UiModelState;
  readonly snapshot: RuntimeSnapshotDto | null;
  readonly overlay:
    | { readonly kind: "commands" | "activity" | "help" | "status" | "sessions" }
    | { readonly kind: "control"; readonly tab: ControlPanelTab }
    | null;
  readonly notice: string | null;
}

export interface AgentTuiControllerOptions {
  readonly launcher: AgentLauncher;
  readonly render: (frame: AgentTuiFrame) => void;
  readonly requestExit?: () => void;
  readonly preferenceStore?: ClientPreferenceStore;
  readonly initialPreferences?: PreferencesState;
}

export function createInitialFrame(preferences?: PreferencesState): AgentTuiFrame {
  const ui = createInitialUiModelState();
  return {
    ui: preferences === undefined ? ui : { ...ui, preferences },
    snapshot: null,
    overlay: null,
    notice: null,
  };
}
