import type { DecisionModelDto } from "@lyonbrown/agent-client";
import { getDaemonMutationAvailability } from "@lyonbrown/ui-model";

import type { AgentTuiFrame } from "@lyonbrown/agent-cli/internal/controller";
import type { SurfaceStyles } from "@lyonbrown/agent-cli/internal/surface-types";
import { safeTerminalText } from "@lyonbrown/agent-cli/internal/text-safety";

const PHASES: Record<DecisionModelDto["phase"], readonly [string, string]> = {
  unavailable: ["Storage not configured", "尚未配置模型目录"],
  checking: ["Checking local weights", "正在检查本地权重"],
  required: ["Not installed", "尚未安装"],
  "waiting-for-lock": ["Waiting for another installation", "正在等待其他安装"],
  downloading: ["Downloading", "正在下载"],
  verifying: ["Verifying", "正在校验"],
  installing: ["Installing", "正在安装"],
  configured: ["Installed · loads on first use", "已安装 · 首次使用时加载"],
  cancelled: ["Cancelled", "已取消"],
  failed: ["Setup failed", "安装失败"],
};

export function decisionModelSummary(frame: AgentTuiFrame): string {
  const zh = frame.ui.preferences.locale === "zh-CN";
  const model = frame.snapshot?.decisionModel;
  const phase =
    model === null || model === undefined
      ? zh
        ? "状态不可用"
        : "Status unavailable"
      : PHASES[model.phase][zh ? 1 : 0];
  return `${zh ? "决策模型" : "Decision model"} · ${phase}`;
}

export function renderDecisionModel(
  frame: AgentTuiFrame,
  width: number,
  styles: SurfaceStyles,
): string[] {
  const zh = frame.ui.preferences.locale === "zh-CN";
  const model = frame.snapshot?.decisionModel;
  const lines = [
    styles.accent(decisionModelSummary(frame)),
    styles.muted(
      zh
        ? "可选的前置分类；聊天始终继续执行。"
        : "Optional preflight classification; chat always continues.",
    ),
  ];
  if (model === null || model === undefined) return lines;
  lines.push(
    styles.secondary(
      `${safeTerminalText(model.providerId)} · ${safeTerminalText(model.modelId)} · q4`,
    ),
  );
  lines.push(
    styles.muted(`${zh ? "权重版本" : "Weights revision"}: ${model.modelRevision.slice(0, 12)}`),
  );
  if (model.phase === "unavailable") {
    lines.push(
      styles.warning(
        zh
          ? "请配置模型存储目录后重新启动。"
          : "Configure the model storage directory and restart.",
      ),
    );
  }
  if (model.phase !== "configured" && model.phase !== "unavailable") {
    const percent = Number((model.downloadedBytes * 100n) / model.totalBytes);
    const cells = width < 60 ? 12 : 24;
    const filled = Math.round((Math.max(0, Math.min(100, percent)) * cells) / 100);
    const mib = (value: bigint) => String(value / 1_048_576n);
    lines.push(
      styles.muted(
        `${"█".repeat(filled)}${"░".repeat(cells - filled)} ${String(percent)}% · ${mib(model.downloadedBytes)}/${mib(model.totalBytes)} MiB`,
      ),
    );
  }
  if (model.failure !== null)
    lines.push(
      styles.danger(
        zh
          ? `! 安装失败 · ${safeTerminalText(model.failure.code)}`
          : `! Setup failed · ${safeTerminalText(model.failure.code)}`,
      ),
    );
  if (!getDaemonMutationAvailability(frame.ui).enabled) {
    lines.push(
      styles.warning(zh ? "连接已过期，操作已禁用。" : "Connection is stale; actions disabled."),
    );
  } else if (model.allowedActions.includes("cancel")) {
    lines.push(styles.active(zh ? "Enter/C  取消安装" : "Enter/C  Cancel installation"));
  } else if (model.allowedActions.includes("download")) {
    lines.push(
      styles.active(
        zh
          ? "Enter/D  下载并安装（仅在确认后）"
          : "Enter/D  Download and install (only after confirmation)",
      ),
    );
  } else if (model.allowedActions.includes("retry")) {
    lines.push(styles.active(zh ? "Enter/R  重试安装" : "Enter/R  Retry installation"));
  }
  return lines;
}

export function decisionPanelAction(frame: AgentTuiFrame): string | null {
  if (!getDaemonMutationAvailability(frame.ui).enabled) return null;
  const actions = frame.snapshot?.decisionModel?.allowedActions ?? [];
  if (actions.includes("cancel")) return "/decision cancel";
  if (actions.includes("download")) return "/decision install";
  if (actions.includes("retry")) return "/decision retry";
  return null;
}
