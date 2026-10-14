export interface DesktopModel {
  readonly purpose: "chat" | "decision";
  readonly title: string;
  readonly phase: string;
  readonly downloadedBytes: string;
  readonly totalBytes: string | null;
  readonly actions: readonly ("download" | "retry" | "cancel")[];
}
export interface DesktopState {
  readonly status: "connecting" | "connected" | "disconnected";
  readonly source: "reused" | "spawned" | null;
  readonly revision: string | null;
  readonly actionToken: string | null;
  readonly phase: string;
  readonly messages: readonly {
    readonly id: string;
    readonly role: "user" | "assistant";
    readonly text: string;
  }[];
  readonly canSend: boolean;
  readonly canCancel: boolean;
  readonly approval: {
    readonly tool: string;
    readonly risk: string;
    readonly target: string;
  } | null;
  readonly models: readonly DesktopModel[];
  readonly error: string | null;
}
export type DesktopAction =
  | { readonly type: "send"; readonly text: string; readonly token: string }
  | { readonly type: "cancel" | "approve" | "deny"; readonly token: string }
  | {
      readonly type: "model";
      readonly purpose: "chat" | "decision";
      readonly action: "download" | "retry" | "cancel";
      readonly token: string;
    };
export interface ActionResult {
  readonly ok: boolean;
  readonly error: string | null;
}
export interface DesktopBridge {
  getState(): Promise<DesktopState>;
  reconnect(): Promise<ActionResult>;
  readonly act: (action: DesktopAction) => Promise<ActionResult>;
  onState(listener: (state: DesktopState) => void): () => void;
}
export const CHANNELS = {
  state: "desktop:state",
  reconnect: "desktop:reconnect",
  action: "desktop:action",
  changed: "desktop:changed",
} as const;
