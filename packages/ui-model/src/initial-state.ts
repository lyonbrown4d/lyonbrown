import type { UiModelState } from "@lyonbrown/ui-model/internal/types";

export const DEFAULT_LOCALE = "en-US" as const;
export const DEFAULT_THEME_ID = "hearth-teal" as const;

export function createInitialUiModelState(): UiModelState {
  return {
    remote: {
      connection: "disconnected",
      serverInstanceId: null,
      snapshot: null,
      snapshotFresh: false,
      unavailableReason: null,
    },
    modelDownloadIntent: null,
    approval: null,
    inspector: {
      selection: null,
      followLatest: true,
      narrowViewMode: "transcript",
    },
    preferences: {
      locale: DEFAULT_LOCALE,
      themeId: DEFAULT_THEME_ID,
    },
  };
}
