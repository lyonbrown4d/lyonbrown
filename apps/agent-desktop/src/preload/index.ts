import { contextBridge, ipcRenderer, type IpcRendererEvent } from "electron";
import {
  CHANNELS,
  type DesktopBridge,
  type DesktopState,
  type ActionResult,
} from "@lyonbrown/agent-desktop/internal/shared/contract";
const bridge: DesktopBridge = {
  getState: () => ipcRenderer.invoke(CHANNELS.state) as Promise<DesktopState>,
  reconnect: () => ipcRenderer.invoke(CHANNELS.reconnect) as Promise<ActionResult>,
  act: (action) => ipcRenderer.invoke(CHANNELS.action, action) as Promise<ActionResult>,
  onState: (listener) => {
    const handler = (_event: IpcRendererEvent, state: DesktopState): void => {
      listener(state);
    };
    ipcRenderer.on(CHANNELS.changed, handler);
    return () => {
      ipcRenderer.removeListener(CHANNELS.changed, handler);
    };
  },
};
contextBridge.exposeInMainWorld("lyonbrown", bridge);
