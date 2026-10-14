import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor, fireEvent } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { DesktopApp } from "@lyonbrown/agent-desktop/internal/renderer/app";
import {
  disconnectedState,
  projectSnapshot,
} from "@lyonbrown/agent-desktop/internal/main/projection";
import { snapshot } from "@lyonbrown/agent-desktop/test/support";
import type { DesktopBridge } from "@lyonbrown/agent-desktop/internal/shared/contract";
afterEach(cleanup);
function fixture(
  state = projectSnapshot(snapshot(), "reused"),
  overrides: Partial<DesktopBridge> = {},
) {
  const bridge: DesktopBridge = {
    getState: async () => state,
    reconnect: async () => ({ ok: true, error: null }),
    act: vi.fn(async () => ({ ok: true, error: null })),
    onState: () => () => {},
    ...overrides,
  };
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <DesktopApp bridge={bridge} />
    </QueryClientProvider>,
  );
  return bridge;
}
test("renders real client state and sends via the restricted bridge", async () => {
  const bridge = fixture();
  await screen.findByText("connected");
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "hello" } });
  fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() =>
    expect(bridge.act).toHaveBeenCalledWith({ type: "send", text: "hello", token: "fixture:1" }),
  );
});
test("shows a reconnect path and disables disconnected submissions", async () => {
  fixture(disconnectedState());
  await screen.findByRole("button", { name: "Reconnect" });
  expect(screen.getByRole("button", { name: "Send" }).hasAttribute("disabled")).toBe(true);
});
test("renders untrusted conversation text as text, not executable HTML", async () => {
  fixture(
    projectSnapshot(
      snapshot({
        turns: [
          {
            turnId: "t",
            userText: "<img onerror=evil()>",
            assistantText: "reply",
            status: "completed",
            steering: [],
            activities: [],
          },
        ],
      }),
      "reused",
    ),
  );
  await screen.findByText("<img onerror=evil()>");
  expect(document.querySelector("img")).toBeNull();
});
test("exposes model progress, purpose-scoped installation and Chinese labels", async () => {
  const state = projectSnapshot(snapshot(), "reused");
  const bridge = fixture({
    ...state,
    models: [
      {
        purpose: "decision",
        title: "KEV",
        phase: "downloading",
        downloadedBytes: "25",
        totalBytes: "100",
        actions: ["cancel"],
      },
    ],
  });
  await screen.findByRole("progressbar");
  fireEvent.click(screen.getByRole("button", { name: "Cancel download" }));
  await waitFor(() =>
    expect(bridge.act).toHaveBeenCalledWith({
      type: "model",
      purpose: "decision",
      action: "cancel",
      token: "fixture:1",
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "中文" }));
  await screen.findByText("决策模型 · 可选");
  expect(screen.getByRole("button", { name: "取消下载" })).toBeDefined();
});
test("sends explicit approval and cancellation actions", async () => {
  const state = projectSnapshot(snapshot({ phase: "awaiting-approval" }), "reused");
  const bridge = fixture({
    ...state,
    approval: { tool: "write", risk: "write", target: "notes.txt" },
  });
  await screen.findByText("Approval required: write");
  fireEvent.click(screen.getByRole("button", { name: "Allow once" }));
  await waitFor(() =>
    expect(bridge.act).toHaveBeenCalledWith({ type: "approve", token: "fixture:1" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Cancel turn" }));
  await waitFor(() =>
    expect(bridge.act).toHaveBeenCalledWith({ type: "cancel", token: "fixture:1" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Deny" }));
  await waitFor(() =>
    expect(bridge.act).toHaveBeenCalledWith({ type: "deny", token: "fixture:1" }),
  );
});
test("keeps input on rejected commands and prevents duplicate pending submissions", async () => {
  let finish!: (value: { ok: boolean; error: string | null }) => void;
  const act = vi.fn(
    () =>
      new Promise<{ ok: boolean; error: string | null }>((resolve) => {
        finish = resolve;
      }),
  );
  fixture(undefined, { act });
  await screen.findByText("connected");
  fireEvent.change(screen.getByLabelText("Message"), { target: { value: "keep me" } });
  const form = screen.getByRole("button", { name: "Send" }).closest("form");
  if (!form) throw new Error("missing form");
  fireEvent.submit(form);
  fireEvent.submit(form);
  await waitFor(() => expect(act).toHaveBeenCalledTimes(1));
  finish({ ok: false, error: "ACTION_REJECTED" });
  await screen.findByRole("alert");
  expect(screen.getByLabelText<HTMLTextAreaElement>("Message").value).toBe("keep me");
});
test("represents an unavailable preload bridge without allowing submission", async () => {
  fixture(undefined, {
    getState: async () => {
      throw new Error("unavailable");
    },
  });
  await screen.findByText("Desktop bridge unavailable. Restart the app.");
  expect(screen.getByRole("button", { name: "Send" }).hasAttribute("disabled")).toBe(true);
});
