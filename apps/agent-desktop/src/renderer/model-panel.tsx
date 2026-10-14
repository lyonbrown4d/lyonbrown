import type { DesktopModel } from "@lyonbrown/agent-desktop/internal/shared/contract";
import styles from "@lyonbrown/agent-desktop/internal/renderer/app.module.css";
export function ModelPanel({
  model,
  chinese,
  disabled,
  onAction,
}: {
  readonly model: DesktopModel;
  readonly chinese: boolean;
  readonly disabled: boolean;
  readonly onAction: (action: "download" | "retry" | "cancel") => void;
}) {
  const total = model.totalBytes === null ? 0 : Number(model.totalBytes);
  const progress =
    total > 0 ? Math.min(100, (Number(model.downloadedBytes) / total) * 100) : undefined;
  const names = {
    download: chinese ? "下载" : "Download",
    retry: chinese ? "重试" : "Retry",
    cancel: chinese ? "取消下载" : "Cancel download",
  };
  return (
    <section className={styles.model}>
      <h2>
        {model.purpose === "chat"
          ? chinese
            ? "聊天模型"
            : "Chat model"
          : chinese
            ? "决策模型 · 可选"
            : "Decision model · optional"}
      </h2>
      <p>{model.title}</p>
      <p>{model.phase}</p>
      {["downloading", "verifying", "installing"].includes(model.phase) && (
        <progress
          aria-label={chinese ? "下载进度" : "Download progress"}
          max={100}
          value={progress}
        />
      )}
      {model.actions.map((action) => (
        <button
          key={action}
          disabled={disabled}
          onClick={() => {
            onAction(action);
          }}
        >
          {names[action]}
        </button>
      ))}
    </section>
  );
}
