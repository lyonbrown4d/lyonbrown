import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { app, BrowserWindow, ipcMain, session, type IpcMainInvokeEvent } from "electron";
import { DesktopProcessBackend } from "@lyonbrown/agent-desktop/internal/main/process-backend";
import { rendererEnvironmentUrl } from "@lyonbrown/agent-desktop/internal/main/environment";
import {
  assertTrustedSender,
  trustedRendererUrl,
} from "@lyonbrown/agent-desktop/internal/main/security";
import { CHANNELS } from "@lyonbrown/agent-desktop/internal/shared/contract";

let backend: DesktopProcessBackend | undefined;
let exiting = false;
async function start(): Promise<void> {
  await app.whenReady();
  session.defaultSession.setPermissionRequestHandler((_contents, _permission, callback) => {
    callback(false);
  });
  session.defaultSession.setPermissionCheckHandler(() => false);
  const window = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 760,
    minHeight: 560,
    title: "Lyonbrown",
    backgroundColor: "#191d24",
    webPreferences: {
      preload: resolve(import.meta.dirname, "../preload/index.cjs"),
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      webSecurity: true,
    },
  });
  const devUrl = rendererEnvironmentUrl();
  const rendererUrl =
    devUrl && !app.isPackaged
      ? trustedRendererUrl(devUrl)
      : pathToFileURL(resolve(import.meta.dirname, "../renderer/index.html")).href;
  const validate = (event: IpcMainInvokeEvent): void => {
    assertTrustedSender(
      {
        owned: event.sender === window.webContents,
        mainFrame: event.senderFrame === window.webContents.mainFrame,
        url: event.senderFrame?.url ?? "",
      },
      rendererUrl,
    );
  };
  backend = new DesktopProcessBackend((state) => {
    if (!window.isDestroyed()) window.webContents.send(CHANNELS.changed, state);
  });
  const ownedBackend = backend;
  ipcMain.handle(CHANNELS.state, (event) => {
    validate(event);
    return ownedBackend.state();
  });
  ipcMain.handle(CHANNELS.reconnect, (event) => {
    validate(event);
    return ownedBackend.connect();
  });
  ipcMain.handle(CHANNELS.action, (event, action: unknown) => {
    validate(event);
    return ownedBackend.act(action);
  });
  window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  window.webContents.on("will-navigate", (event) => {
    event.preventDefault();
  });
  window.webContents.on("will-redirect", (event) => {
    event.preventDefault();
  });
  window.on("closed", () => {
    app.quit();
  });
  await window.loadURL(rendererUrl);
  await ownedBackend.connect();
}
app.on("before-quit", (event) => {
  if (exiting) return;
  event.preventDefault();
  exiting = true;
  void (backend?.close() ?? Promise.resolve()).finally(() => {
    app.quit();
  });
});
void start().catch(() => {
  console.error("DESKTOP_START_FAILED");
  app.quit();
});
