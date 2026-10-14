import { expect, test } from "vitest";
import { resolve } from "node:path";
import { loadDesktopConfiguration } from "@lyonbrown/agent-desktop/internal/main/configuration";
import { trustedRendererUrl, parseAction } from "@lyonbrown/agent-desktop/internal/main/security";
import { CHANNELS } from "@lyonbrown/agent-desktop/internal/shared/contract";
const defaults = { workspace: resolve("."), homeDirectory: resolve("home") };
test("uses typed process configuration and standalone Node for daemon spawning", () => {
  expect(loadDesktopConfiguration({}, defaults)).toEqual({
    ...defaults,
    nodeExecutable: "node",
    autoStart: true,
  });
  expect(
    loadDesktopConfiguration(
      {
        LYONBROWN_DESKTOP_AUTOSTART: "false",
        LYONBROWN_DESKTOP_NODE_EXECUTABLE: "C:\\node\\node.exe",
      },
      defaults,
    ),
  ).toMatchObject({ autoStart: false, nodeExecutable: "C:\\node\\node.exe" });
  for (const env of [
    { LYONBROWN_DESKTOP_WORKSPACE: "relative" },
    { LYONBROWN_DESKTOP_HOME: "relative" },
    { LYONBROWN_DESKTOP_NODE_EXECUTABLE: "" },
    { LYONBROWN_DESKTOP_AUTOSTART: "yes" },
  ])
    expect(() => loadDesktopConfiguration(env, defaults)).toThrow();
});
test("accepts only loopback developer renderers without credentials or fragments", () => {
  expect(trustedRendererUrl("http://localhost:5173")).toBe("http://localhost:5173/");
  for (const url of [
    "https://evil.test",
    "file:///tmp/index.html",
    "http://user:password@localhost/",
    "http://localhost/#other",
  ])
    expect(() => trustedRendererUrl(url)).toThrow();
  expect(CHANNELS.action).toBe("desktop:action");
  expect(
    parseAction({ type: "model", purpose: "chat", action: "retry", token: "fixture:1" }).type,
  ).toBe("model");
  expect(parseAction({ type: "deny", token: "fixture:1" }).type).toBe("deny");
});
