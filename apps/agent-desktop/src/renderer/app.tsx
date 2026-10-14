import { useState, type SyntheticEvent } from "react";
import type { DesktopBridge } from "@lyonbrown/agent-desktop/internal/shared/contract";
import {
  useDesktopActions,
  useDesktopState,
} from "@lyonbrown/agent-desktop/internal/renderer/hooks";
import { ModelPanel } from "@lyonbrown/agent-desktop/internal/renderer/model-panel";
import styles from "@lyonbrown/agent-desktop/internal/renderer/app.module.css";

export function DesktopApp({ bridge }: { readonly bridge: DesktopBridge }) {
  const state = useDesktopState(bridge);
  const actions = useDesktopActions(bridge);
  const [text, setText] = useState("");
  const [chinese, setChinese] = useState(false);
  const view = state.data;
  const label = (en: string, zh: string): string => (chinese ? zh : en);
  const submit = async (event: SyntheticEvent): Promise<void> => {
    event.preventDefault();
    if (!view?.canSend || !view.actionToken || actions.pending || !text.trim()) return;
    if (await actions.execute({ type: "send", text, token: view.actionToken })) setText("");
  };
  return (
    <div className={styles.shell}>
      <header className={styles.header}>
        <div>
          <strong>Lyonbrown</strong>
          <span>Desktop · {label("Workspace client", "工作区客户端")}</span>
        </div>
        <button
          onClick={() => {
            setChinese(!chinese);
          }}
        >
          {chinese ? "English" : "中文"}
        </button>
      </header>
      <aside className={styles.sidebar}>
        <h2>{label("Connection", "连接")}</h2>
        <p role="status">{view?.status ?? label("Connecting…", "连接中…")}</p>
        {view && (
          <p>
            {view.phase} · {view.source ?? "—"}
          </p>
        )}
        {view?.status === "disconnected" && (
          <button
            disabled={actions.pending}
            onClick={() => {
              void actions.execute("reconnect");
            }}
          >
            {label("Reconnect", "重新连接")}
          </button>
        )}
        {view?.models.map((model) => (
          <ModelPanel
            key={model.purpose}
            model={model}
            chinese={chinese}
            disabled={actions.pending || view.status !== "connected"}
            onAction={(action) => {
              if (view.actionToken)
                void actions.execute({
                  type: "model",
                  purpose: model.purpose,
                  action,
                  token: view.actionToken,
                });
            }}
          />
        ))}
        <p className={styles.hint}>
          {label(
            "Models and tools run in agentd. Downloads start only when you choose them.",
            "模型和工具由 agentd 执行。仅在你选择后开始下载。",
          )}
        </p>
      </aside>
      <main className={styles.conversation}>
        <h1>{label("Conversation", "会话")}</h1>
        {state.isPending && <p role="status">{label("Connecting to backend…", "正在连接后端…")}</p>}
        {state.isError && (
          <p role="alert">
            {label(
              "Desktop bridge unavailable. Restart the app.",
              "桌面接口不可用，请重新启动应用。",
            )}
          </p>
        )}
        {(view?.error ?? actions.error) && <p role="alert">{view?.error ?? actions.error}</p>}
        <div className={styles.transcript} aria-live="polite">
          {view?.messages.length === 0 && (
            <p className={styles.empty}>
              {label(
                "Start a conversation when your chat model is ready.",
                "聊天模型就绪后即可开始会话。",
              )}
            </p>
          )}
          {view?.messages.map((message) => (
            <article key={message.id}>
              <h2>{message.role === "user" ? label("You", "你") : "Lyonbrown"}</h2>
              <p>{message.text || "…"}</p>
            </article>
          ))}
        </div>
        {view?.approval && (
          <section className={styles.approval} aria-label={label("Approval", "审批")}>
            <h2>
              {label("Approval required", "需要审批")}: {view.approval.tool}
            </h2>
            <p>
              {view.approval.risk} · {view.approval.target}
            </p>
            {(["approve", "deny"] as const).map((type) => (
              <button
                key={type}
                disabled={actions.pending || view.status !== "connected"}
                onClick={() => {
                  if (view.actionToken) void actions.execute({ type, token: view.actionToken });
                }}
              >
                {type === "approve" ? label("Allow once", "允许一次") : label("Deny", "拒绝")}
              </button>
            ))}
          </section>
        )}
        <form
          className={styles.composer}
          onSubmit={(event) => {
            void submit(event);
          }}
        >
          <label htmlFor="message">{label("Message", "消息")}</label>
          <textarea
            id="message"
            value={text}
            onChange={(event) => {
              setText(event.target.value);
            }}
            disabled={!view?.canSend || actions.pending}
            rows={3}
            placeholder={label("Ask Lyonbrown…", "向 Lyonbrown 提问…")}
          />
          <div>
            <span>{label("Enter creates a new line", "Enter 换行")}</span>
            {view?.canCancel && (
              <button
                type="button"
                disabled={actions.pending || view.status !== "connected"}
                onClick={() => {
                  if (view.actionToken)
                    void actions.execute({ type: "cancel", token: view.actionToken });
                }}
              >
                {label("Cancel turn", "取消执行")}
              </button>
            )}
            <button type="submit" disabled={!view?.canSend || actions.pending || !text.trim()}>
              {label("Send", "发送")}
            </button>
          </div>
        </form>
      </main>
    </div>
  );
}
