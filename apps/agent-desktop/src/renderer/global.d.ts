import type { DesktopBridge } from "@lyonbrown/agent-desktop/internal/shared/contract";
declare global {
  interface Window {
    readonly lyonbrown: DesktopBridge;
  }
}
