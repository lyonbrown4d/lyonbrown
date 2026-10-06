import type { ModelSetupDto } from "@lyonbrown/agent-client";
import type { Locale } from "@lyonbrown/ui-model";

import { labels } from "./locales.ts";
import { safeTerminalText } from "./text-safety.ts";

export interface ModelSetupStyles {
  readonly heading: (value: string) => string;
  readonly muted: (value: string) => string;
  readonly warning: (value: string) => string;
  readonly danger: (value: string) => string;
}

export interface RenderModelSetupOptions {
  readonly setup: ModelSetupDto;
  readonly locale: Locale;
  readonly width: number;
  readonly mutationsEnabled: boolean;
  readonly styles: ModelSetupStyles;
}

export function renderModelSetup(options: RenderModelSetupOptions): string[] {
  const { setup, locale, width, mutationsEnabled, styles } = options;
  const text = labels(locale);
  const model = setup.managedModel;
  const lines: string[] = [];
  const modelName = modelTitle(model, width, setup.source);
  lines.push(styles.heading(`${text.model}${width < 52 ? " · " : "  "}${modelName}`));

  if (setup.phase === "required" && model !== null) {
    lines.push(
      styles.muted(
        `${recommendationLabel(model.recommendationReason, text.modelSetup)} · ${formatBytes(model.expectedDownloadBytes)} · ${text.modelSetup.confirmDownload}`,
      ),
    );
  } else {
    lines.push(renderPhase(setup, text.modelSetup.phase[setup.phase], width, styles));
    if (isTransferPhase(setup.phase)) lines.push(renderProgress(setup, width, styles));
  }

  if (setup.failure !== null) {
    lines.push(styles.danger(`! ${safeTerminalText(setup.failure.message)}`));
  }

  lines.push(
    mutationsEnabled
      ? renderActions(setup, text.modelSetup, width, styles)
      : styles.warning(`! ${text.modelSetup.actionsDisabled} · Q ${text.modelSetup.exit}`),
  );
  return lines;
}

function modelTitle(
  model: ModelSetupDto["managedModel"],
  width: number,
  fallback: ModelSetupDto["source"],
): string {
  if (model === null) return fallback;
  const id = safeTerminalText(model.id);
  const displayName = model.displayName === null ? "" : safeTerminalText(model.displayName);
  if (displayName.length === 0 || width < 52) return id;
  return `${displayName} · ${id}`;
}

function recommendationLabel(
  reason: NonNullable<ModelSetupDto["managedModel"]>["recommendationReason"],
  text: ReturnType<typeof labels>["modelSetup"],
): string {
  switch (reason) {
    case "installed-preferred":
      return text.installedPreferred;
    case "hardware-compatible":
      return text.hardwareCompatible;
    case "conservative-fallback":
      return text.conservativeFallback;
    case null:
      return text.recommended;
  }
}

function renderPhase(
  setup: ModelSetupDto,
  phase: string,
  width: number,
  styles: ModelSetupStyles,
): string {
  const operation = setup.operationId === null ? "" : safeTerminalText(setup.operationId);
  if (operation.length === 0 || width < 52) return styles.warning(`● ${phase}`);
  return styles.warning(`● ${phase} · ${operation}`);
}

function renderProgress(setup: ModelSetupDto, width: number, styles: ModelSetupStyles): string {
  const total = setup.totalBytes;
  if (total === null || total <= 0n) {
    return styles.muted(`◇ ${formatBytes(setup.downloadedBytes)}`);
  }
  const percent = Number((setup.downloadedBytes * 100n) / total);
  const ratio = Math.max(0, Math.min(100, percent));
  const amount =
    width < 52
      ? `${formatCompactMiB(setup.downloadedBytes)}/${formatCompactMiB(total)} MiB`
      : `${formatBytes(setup.downloadedBytes)} / ${formatBytes(total)}`;
  if (width < 40) return styles.muted(`${amount}  ${String(ratio)}%`);
  const cells = width >= 90 ? 28 : 12;
  const filled = Math.round((ratio / 100) * cells);
  return styles.muted(
    `${"█".repeat(filled)}${"░".repeat(cells - filled)}  ${amount}  ${String(ratio)}%`,
  );
}

function renderActions(
  setup: ModelSetupDto,
  text: ReturnType<typeof labels>["modelSetup"],
  width: number,
  styles: ModelSetupStyles,
): string {
  const actions: string[] = [];
  if (setup.allowedActions.includes("download")) actions.push(`Enter/D  ${text.download}`);
  if (setup.allowedActions.includes("retry")) actions.push(`Enter/D/R  ${text.retry}`);
  if (setup.allowedActions.includes("cancel") && setup.operationId !== null) {
    actions.push(`C/Esc  ${text.cancel}`);
  }
  const terminal =
    setup.phase === "required" || setup.phase === "cancelled" || setup.phase === "failed";
  actions.push(`${terminal ? "Q/Esc" : "Q"}  ${text.exit}`);
  return styles.heading(actions.join(width < 52 ? " · " : "     "));
}

function isTransferPhase(phase: ModelSetupDto["phase"]): boolean {
  return (
    phase === "waiting-for-lock" ||
    phase === "checking" ||
    phase === "downloading" ||
    phase === "verifying" ||
    phase === "installing"
  );
}

function formatBytes(bytes: bigint): string {
  const gib = Number(bytes) / 1024 ** 3;
  if (gib >= 1) return `${gib.toFixed(2)} GiB`;
  return `${String(Math.floor(Number(bytes) / 1024 ** 2))} MiB`;
}

function formatCompactMiB(bytes: bigint): string {
  return String(bytes / (1024n * 1024n));
}
