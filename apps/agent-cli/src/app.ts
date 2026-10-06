import type { AgentLauncher } from "@lyonbrown/agent-launch";

import {
  detectClientLocale,
  type ClientPreferenceStore,
} from "@lyonbrown/agent-cli/internal/client-preferences";
import { AgentTuiController, createInitialFrame } from "@lyonbrown/agent-cli/internal/controller";
import { createPiScreen, type PiScreen } from "@lyonbrown/agent-cli/internal/pi-screen";

export interface RunAgentTuiOptions {
  readonly launcher: AgentLauncher;
  readonly preferenceStore?: ClientPreferenceStore;
}

export async function runAgentTui(options: RunAgentTuiOptions): Promise<void> {
  let finish: (() => void) | undefined;
  const finished = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const screenRef: { current?: PiScreen } = {};
  const savedPreferences = await options.preferenceStore?.load();
  const defaults = createInitialFrame().ui.preferences;
  const initialPreferences = savedPreferences ?? {
    ...defaults,
    locale: detectClientLocale(Intl.DateTimeFormat().resolvedOptions().locale),
  };
  const requestExit = (): void => {
    screenRef.current?.stop();
    void controller.close().finally(() => finish?.());
  };
  const controller = new AgentTuiController({
    launcher: options.launcher,
    render: (frame) => screenRef.current?.update(frame),
    requestExit,
    initialPreferences,
    ...(options.preferenceStore === undefined ? {} : { preferenceStore: options.preferenceStore }),
  });
  const screen: PiScreen = createPiScreen({
    frame: createInitialFrame(initialPreferences),
    onSubmit: (text) => controller.submit(text),
    onLanguageToggle: () => {
      controller.toggleLanguage();
    },
    onPaletteToggle: () => {
      controller.togglePalette();
    },
    onActivityToggle: () => {
      controller.toggleActivity();
    },
    onOverlayClose: () => {
      controller.closeOverlay();
    },
    onThemePreview: (themeId) => {
      controller.previewTheme(themeId);
    },
    onExit: requestExit,
  });
  screenRef.current = screen;
  screen.start();
  controller.open();
  await finished;
}
