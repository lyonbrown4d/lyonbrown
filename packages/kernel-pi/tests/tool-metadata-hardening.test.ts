import { describe, expect, test, vi } from "vitest";

import {
  createPiKernelSession,
  type KernelModelAdapter,
  type KernelToolDefinition,
  type PiKernelSessionOptions,
} from "../src/index.ts";

const model: KernelModelAdapter = {
  model: { provider: "offline", id: "metadata-hardening" },
  async *stream() {
    await Promise.resolve();
    yield { type: "done", reason: "stop" };
  },
};

const baseTool: KernelToolDefinition = {
  name: "safe_tool",
  label: "Safe tool",
  description: "safe",
  inputSchema: { type: "object", additionalProperties: false },
  permissions: [],
  risk: "low",
  execute: () => Promise.resolve({ content: "ok" }),
};

function options(tool: KernelToolDefinition): PiKernelSessionOptions {
  return {
    sessionId: "metadata-hardening",
    systemPrompt: "safe",
    model,
    toolRegistry: {
      list: () => [tool],
      get: () => tool,
      hasPermission: () => true,
    },
    approvalGate: { request: () => Promise.resolve({ decision: "approve" }) },
    eventSink: { append: () => Promise.resolve() },
    secretResolver: { resolve: () => Promise.resolve("secret") },
    budget: {
      maxInputBytes: 1_024,
      maxTurns: 2,
      maxSteps: 2,
      maxOutputBytes: 1_024,
      maxToolResultBytes: 256,
      maxValueDepth: 5,
      maxValueStringBytes: 256,
      maxCollectionEntries: 16,
      maxValueNodes: 128,
      toolTimeoutMs: 100,
    },
  };
}

describe("tool metadata hardening", () => {
  test("rejects a proxied definition without invoking its traps", () => {
    const get = vi.fn(() => "must not run");
    const hostile = new Proxy(baseTool, { get });

    expect(() => createPiKernelSession(options(hostile))).toThrow("Proxy");
    expect(get).not.toHaveBeenCalled();
  });

  test("rejects definition and permission accessors without invoking them", () => {
    const nameGetter = vi.fn(() => "unsafe");
    const hostileDefinition = Object.defineProperty({ ...baseTool }, "name", {
      enumerable: true,
      get: nameGetter,
    });
    expect(() => createPiKernelSession(options(hostileDefinition))).toThrow("accessor");
    expect(nameGetter).not.toHaveBeenCalled();

    const permissionGetter = vi.fn(() => "unsafe");
    const permissions: string[] = [];
    Object.defineProperty(permissions, 0, { enumerable: true, get: permissionGetter });
    permissions.length = 1;
    expect(() => createPiKernelSession(options({ ...baseTool, permissions }))).toThrow("accessor");
    expect(permissionGetter).not.toHaveBeenCalled();
  });
});
