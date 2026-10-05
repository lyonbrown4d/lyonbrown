import type { AgentLauncher } from "@lyonbrown/agent-launch";

import { AgentTuiController, createInitialFrame } from "./controller.ts";
import { createPiScreen, type PiScreen } from "./pi-screen.ts";

export interface RunAgentTuiOptions {
  readonly launcher: AgentLauncher;
}

export async function runAgentTui(options: RunAgentTuiOptions): Promise<void> {
  let finish: (() => void) | undefined;
  const finished = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const screenRef: { current?: PiScreen } = {};
  const controller = new AgentTuiController({
    launcher: options.launcher,
    render: (frame) => screenRef.current?.update(frame),
  });
  const screen: PiScreen = createPiScreen({
    frame: createInitialFrame(),
    onSubmit: (text) => void controller.submit(text),
    onLanguageToggle: () => {
      controller.toggleLanguage();
    },
    onPaletteToggle: () => {
      controller.togglePalette();
    },
    onActivityToggle: () => {
      controller.toggleActivity();
    },
    onExit: () => {
      screen.stop();
      void controller.close().finally(() => finish?.());
    },
  });
  screenRef.current = screen;
  screen.start();
  controller.open();
  await finished;
}
