import type { DesktopAction } from "@lyonbrown/agent-desktop/internal/shared/contract";
export function assertTrustedSender(
  sender: { readonly owned: boolean; readonly mainFrame: boolean; readonly url: string },
  expectedUrl: string,
): void {
  if (!sender.owned || !sender.mainFrame || sender.url !== expectedUrl)
    throw new Error("UNTRUSTED_RENDERER");
}
export function parseAction(value: unknown): DesktopAction {
  if (
    typeof value !== "object" ||
    value === null ||
    !("type" in value) ||
    !("token" in value) ||
    typeof value.token !== "string" ||
    !/^[a-zA-Z0-9_-]{1,64}:[0-9]{1,20}$/u.test(value.token)
  )
    throw new Error("INVALID_ACTION");
  const token = value.token;
  if (
    value.type === "send" &&
    "text" in value &&
    typeof value.text === "string" &&
    value.text.length <= 65536 &&
    value.text.isWellFormed() &&
    value.text.trim().length > 0 &&
    !value.text.includes("\0") &&
    new TextEncoder().encode(value.text).byteLength <= 65536
  )
    return { type: "send", text: value.text, token };
  if (value.type === "cancel" || value.type === "approve" || value.type === "deny")
    return { type: value.type, token };
  if (
    value.type === "model" &&
    "purpose" in value &&
    (value.purpose === "chat" || value.purpose === "decision") &&
    "action" in value &&
    (value.action === "download" || value.action === "retry" || value.action === "cancel")
  )
    return { type: "model", purpose: value.purpose, action: value.action, token };
  throw new Error("INVALID_ACTION");
}
export function trustedRendererUrl(value: string): string {
  const url = new URL(value);
  if (
    url.username ||
    url.password ||
    url.hash ||
    !["http:", "https:"].includes(url.protocol) ||
    !["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)
  )
    throw new Error("INVALID_RENDERER_URL");
  return url.href;
}
