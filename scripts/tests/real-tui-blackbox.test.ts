import type { ChildProcess } from "node:child_process";
import {
  ControlPlaneError,
  createDiscoveryDescriptor,
  type SecureRunDirectory,
} from "@lyonbrown/local-control-plane";
import { describe, expect, it, vi } from "vitest";

import {
  TerminalTranscript,
  inspectTuiRender,
  requireTuiRender,
} from "@lyonbrown/workspace/internal/real-tui-blackbox";
import { waitForDiscovery } from "@lyonbrown/workspace/internal/real-tui-blackbox-process";

describe("real TUI black-box evidence", () => {
  it("requires ANSI rendering, connected chrome, and daemon-projected model setup", () => {
    const transcript = new TerminalTranscript(4_096);
    transcript.append("\u001b[2J\u001b[HLYONBROWN  Ready  │  workspace  │  AUTO\r\n");
    transcript.append(
      "\u001b[38;2;1;2;3mLocal model\u001b[0m  Qwen3 1.7B\r\nRecommended for this hardware\r\n",
    );

    expect(inspectTuiRender(transcript)).toEqual({
      ansiRendered: true,
      chromeRendered: true,
      grpcReadyRendered: true,
      modelSetupRendered: true,
    });
    expect(() => requireTuiRender(transcript)).not.toThrow();
  });

  it("accepts the Chinese rendering contract", () => {
    const transcript = new TerminalTranscript(4_096);
    transcript.append("\u001b[HLYONBROWN  已就绪\r\n本地模型  Qwen3 1.7B\r\n根据当前硬件推荐\r\n");

    expect(() => requireTuiRender(transcript)).not.toThrow();
  });

  it("accepts the conservative model recommendation selected by hardware detection", () => {
    const transcript = new TerminalTranscript(4_096);
    transcript.append(
      "\u001b[HLYONBROWN  Ready\r\nLocal model  Qwen3 0.6B\r\nConservative compatibility choice\r\n",
    );

    expect(() => requireTuiRender(transcript)).not.toThrow();
  });

  it("rejects a static banner that never completed a real terminal render", () => {
    const transcript = new TerminalTranscript(4_096);
    transcript.append("LYONBROWN Ready Local model Recommended for this hardware");

    expect(() => requireTuiRender(transcript)).toThrow("ANSI");
  });

  it("bounds captured terminal output without retaining oversized data", () => {
    const transcript = new TerminalTranscript(8);
    transcript.append("1234");
    transcript.append("56789");

    expect(transcript.overflowed).toBe(true);
    expect(transcript.raw).toBe("12345678");
    expect(() => requireTuiRender(transcript)).toThrow("capture limit");
  });

  it("retries a transient Windows discovery read race within the startup deadline", async () => {
    const descriptor = createDiscoveryDescriptor({
      endpoint: "127.0.0.1:43123",
      pid: 42,
      scopeId: "a".repeat(64),
    });
    const read = vi
      .fn<() => Promise<typeof descriptor>>()
      .mockRejectedValueOnce(
        new ControlPlaneError("PLATFORM_SECURITY_FAILURE", { operation: "read_descriptor" }),
      )
      .mockResolvedValue(descriptor);
    const close = vi.fn<() => Promise<void>>().mockResolvedValue(undefined);
    const directory = {
      discovery: { read },
      close,
    } as unknown as SecureRunDirectory;
    const daemon = { exitCode: null, signalCode: null } as ChildProcess;

    await expect(
      waitForDiscovery(
        () => Promise.resolve(directory),
        daemon,
        new Promise(() => undefined),
        1_000,
      ),
    ).resolves.toBe(descriptor);
    expect(read).toHaveBeenCalledTimes(2);
    expect(close).toHaveBeenCalledTimes(2);
  });
});
