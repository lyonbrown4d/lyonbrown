import { expect, test } from "vitest";
import { assertTrustedSender, parseAction } from "@lyonbrown/agent-desktop/internal/main/security";
test("allows only the exact owned top-level renderer", () => {
  expect(() =>
    assertTrustedSender(
      { owned: true, mainFrame: true, url: "file:///app/index.html" },
      "file:///app/index.html",
    ),
  ).not.toThrow();
  for (const sender of [
    { owned: false, mainFrame: true, url: "file:///app/index.html" },
    { owned: true, mainFrame: false, url: "file:///app/index.html" },
    { owned: true, mainFrame: true, url: "https://evil.test/" },
  ]) {
    expect(() => assertTrustedSender(sender, "file:///app/index.html")).toThrow();
  }
});
test("validates bounded commands instead of exposing generic gRPC or IPC", () => {
  expect(parseAction({ type: "send", text: "你好", token: "fixture:1" })).toEqual({
    type: "send",
    text: "你好",
    token: "fixture:1",
  });
  for (const action of [
    null,
    {},
    { type: "force-stop", token: "fixture:1" },
    { type: "send", text: "", token: "fixture:1" },
    { type: "send", text: "x".repeat(65537), token: "fixture:1" },
    { type: "send", text: "ok", token: "-1" },
    { type: "model", purpose: "remote", action: "download", token: "fixture:1" },
  ]) {
    expect(() => parseAction(action)).toThrow();
  }
});
