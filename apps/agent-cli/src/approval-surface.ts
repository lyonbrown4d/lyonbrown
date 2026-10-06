import type { ApprovalSnapshotDto } from "@lyonbrown/agent-client";
import type { Locale } from "@lyonbrown/ui-model";

import type { SurfaceStyles } from "@lyonbrown/agent-cli/internal/surface-types";
import { safeTerminalText } from "@lyonbrown/agent-cli/internal/text-safety";

export function renderApproval(
  approval: ApprovalSnapshotDto,
  locale: Locale,
  styles: SurfaceStyles,
  selection = 0,
): string[] {
  const zh = locale === "zh-CN";
  const preview = approval.preview;
  const lines = [
    styles.danger(
      `${zh ? "需要授权" : "Approval required"} · ${safeTerminalText(approval.toolName)}`,
    ),
  ];
  if (preview !== null) {
    lines.push(
      styles.danger(
        `${riskLabel(preview.risk, locale)} · ${safeTerminalText(preview.risk)} operation`,
      ),
      `${zh ? "动作" : "Action"}      ${safeTerminalText(preview.action)}`,
      `${zh ? "工作目录" : "Working dir"} ${safeTerminalText(preview.workingDirectory ?? "—")}`,
      `${zh ? "目标" : "Target"}      ${safeTerminalText(preview.target)}`,
      `${zh ? "影响范围" : "Impact"}      ${safeTerminalText(preview.impact)}`,
      `${zh ? "隔离状态" : "Sandbox"}     ${sandboxLabel(preview.sandbox)}`,
      `${zh ? "凭据" : "Secrets"}     ${preview.requiredSecrets.length === 0 ? (zh ? "无" : "none") : preview.requiredSecrets.map(safeTerminalText).join(", ")}`,
    );
  } else {
    lines.push(
      styles.warning(
        zh ? "! 无法展示完整预览；默认拒绝" : "! Preview unavailable; deny by default",
      ),
    );
  }
  const actions = [
    `D  ${zh ? "拒绝一次（默认）" : "Deny once (default)"}`,
    `A  ${zh ? "允许一次" : "Allow once"}`,
    `S  ${zh ? `本会话允许 ${safeTerminalText(approval.toolName)}` : `Allow ${safeTerminalText(approval.toolName)} for session`}`,
    `X  ${zh ? "拒绝后续高风险操作" : "Deny remaining risky actions"}`,
  ];
  lines.push(
    ...actions.map((action, index) =>
      index === selection ? styles.active(`› ${action}`) : `  ${action}`,
    ),
    styles.muted(
      zh
        ? "Enter 选择 · ↑↓ 移动 · Ctrl-O 完整详情 · Ctrl-C 拒绝并取消"
        : "Enter select · ↑↓ move · Ctrl-O full details · Ctrl-C deny and cancel",
    ),
  );
  return lines;
}

function riskLabel(
  risk: NonNullable<ApprovalSnapshotDto["preview"]>["risk"],
  locale: Locale,
): string {
  const high = risk === "write" || risk === "execute" || risk === "network";
  if (locale === "zh-CN") return high ? "高风险" : "只读";
  return high ? "High risk" : "Read only";
}

function sandboxLabel(sandbox: NonNullable<ApprovalSnapshotDto["preview"]>["sandbox"]): string {
  switch (sandbox) {
    case "host-unsandboxed":
      return "HOST / unsandboxed";
    case "workspace-restricted":
      return "workspace / restricted";
    case "unknown":
      return "unknown / deny by default";
  }
}
