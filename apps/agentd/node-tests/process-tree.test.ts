import { spawn, type ChildProcess } from "node:child_process";
import { EventEmitter } from "node:events";

import { afterEach, describe, expect, it, vi } from "vitest";

import { terminateProcessTree } from "@lyonbrown/agentd/internal/process-tree";

vi.mock("node:child_process", () => ({ spawn: vi.fn() }));

const originalPlatform = Object.getOwnPropertyDescriptor(process, "platform");

function platform(value: NodeJS.Platform): void {
  Object.defineProperty(process, "platform", { configurable: true, value });
}

function fakeChild(pid: number | undefined): ChildProcess & { kill: ReturnType<typeof vi.fn> } {
  return { pid, kill: vi.fn() } as unknown as ChildProcess & { kill: ReturnType<typeof vi.fn> };
}

describe("process tree termination", () => {
  afterEach(() => {
    if (originalPlatform !== undefined)
      Object.defineProperty(process, "platform", originalPlatform);
    vi.restoreAllMocks();
    vi.mocked(spawn).mockReset();
  });

  it("targets the POSIX process group and falls back to the direct child", () => {
    platform("linux");
    const kill = vi.spyOn(process, "kill").mockImplementation(() => true);
    const child = fakeChild(41);
    terminateProcessTree(child);
    expect(kill).toHaveBeenCalledWith(-41, "SIGKILL");
    expect(child.kill).not.toHaveBeenCalled();

    kill.mockImplementation(() => {
      throw new Error("group unavailable");
    });
    terminateProcessTree(child);
    expect(child.kill).toHaveBeenCalledWith("SIGKILL");
  });

  it("uses System32 taskkill with an argument array and falls back after failure", () => {
    platform("win32");
    const child = fakeChild(42);
    const killer = new EventEmitter() as ChildProcess;
    vi.mocked(spawn).mockReturnValue(killer);
    terminateProcessTree(child);
    expect(spawn).toHaveBeenCalledWith(
      expect.stringMatching(/System32[/\\]taskkill\.exe$/u),
      ["/PID", "42", "/T", "/F"],
      { stdio: "ignore", windowsHide: true },
    );
    killer.emit("close", 1);
    expect(child.kill).toHaveBeenCalledOnce();
  });

  it("does not invoke an OS killer when no child PID exists", () => {
    platform("win32");
    const child = fakeChild(undefined);
    terminateProcessTree(child);
    expect(spawn).not.toHaveBeenCalled();
    expect(child.kill).not.toHaveBeenCalled();
  });
});
