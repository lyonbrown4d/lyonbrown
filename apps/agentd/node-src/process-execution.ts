import { spawn } from "node:child_process";

import type { Root } from "@openclaw/fs-safe/root";
import type { KernelToolDefinition, KernelToolExecutionContext } from "@lyonbrown/kernel-pi";

import { terminateProcessTree } from "@lyonbrown/agentd/internal/process-tree";
import { toRelative } from "@lyonbrown/agentd/internal/workspace-paths";

// JSON escaping can expand each captured byte; two streams still fit the kernel's 64 KiB result cap.
const MAX_OUTPUT_BYTES = 4 * 1024;
const MAX_DURATION_MS = 120_000;
const INHERITED_ENV = [
  "PATH",
  "HOME",
  "USERPROFILE",
  "APPDATA",
  "LOCALAPPDATA",
  "TMPDIR",
  "TMP",
  "TEMP",
  "SystemRoot",
  "WINDIR",
  "PATHEXT",
  "LANG",
  "LC_ALL",
  "LC_CTYPE",
] as const;
const FORBIDDEN_ENV =
  /(?:SECRET|TOKEN|PASSWORD|CREDENTIAL|PRIVATE|AUTH|KEY|^NODE_OPTIONS$|^NODE_PATH$)/iu;

function argument(value: unknown, maxLength: number, label: string, allowEmpty = false): string {
  if (
    typeof value !== "string" ||
    (!allowEmpty && value.length === 0) ||
    value.length > maxLength ||
    value.includes("\0")
  ) {
    throw new Error(`${label} invalid`);
  }
  return value;
}

function environment(additions: unknown): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const key of INHERITED_ENV) {
    const inheritedKey =
      process.platform === "win32" && key === "PATH"
        ? Object.keys(process.env).find((candidate) => candidate.toUpperCase() === "PATH")
        : key;
    if (inheritedKey === undefined) continue;
    const value = process.env[inheritedKey];
    if (value !== undefined) env[inheritedKey] = value;
  }
  if (additions === undefined) return env;
  if (typeof additions !== "object" || additions === null || Array.isArray(additions)) {
    throw new Error("process environment invalid");
  }
  const entries = Object.entries(additions);
  if (entries.length > 16) throw new Error("process environment invalid");
  const seen = new Set<string>();
  for (const [key, value] of entries) {
    if (
      !/^[A-Za-z_][A-Za-z0-9_]{0,63}$/u.test(key) ||
      FORBIDDEN_ENV.test(key) ||
      ["__proto__", "prototype", "constructor"].includes(key)
    ) {
      throw new Error("process environment key denied");
    }
    const folded = process.platform === "win32" ? key.toUpperCase() : key;
    if (seen.has(folded)) throw new Error("duplicate process environment key");
    seen.add(folded);
    if (process.platform === "win32") {
      const previous = Object.keys(env).find((candidate) => candidate.toUpperCase() === folded);
      if (previous !== undefined) env[previous] = undefined;
    }
    env[key] = argument(value, 2_048, "process environment value", true);
  }
  return env;
}

function capture(chunks: Buffer[], data: Buffer, length: number): number {
  const remaining = MAX_OUTPUT_BYTES - length;
  if (remaining > 0) chunks.push(data.subarray(0, remaining));
  return length + data.byteLength;
}

async function execute(root: Root, context: KernelToolExecutionContext) {
  const input = context.arguments;
  const executable = argument(input.executable, 4_096, "executable");
  if (!Array.isArray(input.args) || input.args.length > 128) throw new Error("arguments invalid");
  const args = input.args.map((value: unknown) => argument(value, 8_192, "argument", true));
  const cwdInput = input.cwd === undefined ? "." : argument(input.cwd, 4_096, "working directory");
  const cwd = await root.resolve(toRelative(root, cwdInput));
  if (!(await root.stat(toRelative(root, cwd))).isDirectory)
    throw new Error("working directory invalid");
  const timeoutMs = input.timeoutMs === undefined ? 30_000 : input.timeoutMs;
  if (
    typeof timeoutMs !== "number" ||
    !Number.isInteger(timeoutMs) ||
    timeoutMs < 1 ||
    timeoutMs > MAX_DURATION_MS
  ) {
    throw new Error("timeout invalid");
  }
  const env = environment(input.env);
  context.signal.throwIfAborted();
  return new Promise<{ content: string }>((resolve, reject) => {
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    let stdoutBytes = 0;
    let stderrBytes = 0;
    let reason: "timeout" | "cancelled" | "output limit" | undefined;
    let settled = false;
    let terminationWatchdog: ReturnType<typeof setTimeout> | undefined;
    let child;
    try {
      child = spawn(executable, args, {
        cwd,
        env,
        shell: false,
        detached: process.platform !== "win32",
        windowsHide: true,
        stdio: ["ignore", "pipe", "pipe"],
      });
    } catch {
      reject(new Error("process could not start"));
      return;
    }
    const stop = (why: "timeout" | "cancelled" | "output limit") => {
      if (reason !== undefined) return;
      reason = why;
      terminateProcessTree(child);
      terminationWatchdog = setTimeout(() => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        context.signal.removeEventListener("abort", onAbort);
        child.stdout.destroy();
        child.stderr.destroy();
        reject(new Error("process termination unconfirmed"));
      }, 2_000);
    };
    const onAbort = () => {
      stop("cancelled");
    };
    const timer = setTimeout(() => {
      stop("timeout");
    }, timeoutMs);
    context.signal.addEventListener("abort", onAbort, { once: true });
    if (context.signal.aborted) onAbort();
    child.stdout.on("data", (data: Buffer) => {
      stdoutBytes = capture(stdout, data, stdoutBytes);
      if (stdoutBytes > MAX_OUTPUT_BYTES) stop("output limit");
    });
    child.stderr.on("data", (data: Buffer) => {
      stderrBytes = capture(stderr, data, stderrBytes);
      if (stderrBytes > MAX_OUTPUT_BYTES) stop("output limit");
    });
    child.once("error", () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (terminationWatchdog !== undefined) clearTimeout(terminationWatchdog);
      context.signal.removeEventListener("abort", onAbort);
      reject(new Error("process could not start"));
    });
    child.once("close", (code, signal) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (terminationWatchdog !== undefined) clearTimeout(terminationWatchdog);
      context.signal.removeEventListener("abort", onAbort);
      resolve({
        content: JSON.stringify({
          exitCode: code,
          signal,
          reason: reason ?? null,
          stdout: Buffer.concat(stdout).toString("utf8"),
          stderr: Buffer.concat(stderr).toString("utf8"),
          stdoutTruncated: stdoutBytes > MAX_OUTPUT_BYTES,
          stderrTruncated: stderrBytes > MAX_OUTPUT_BYTES,
        }),
      });
    });
  });
}

export function createProcessExecuteTool(root: Root): KernelToolDefinition {
  return Object.freeze({
    name: "process_execute",
    label: "Execute process",
    description:
      "Run one executable with an argument array from the workspace. Requires approval and has host user authority.",
    inputSchema: {
      type: "object",
      properties: {
        executable: { type: "string", minLength: 1, maxLength: 4_096 },
        args: { type: "array", items: { type: "string", maxLength: 8_192 }, maxItems: 128 },
        cwd: { type: "string", minLength: 1, maxLength: 4_096 },
        timeoutMs: { type: "integer", minimum: 1, maximum: MAX_DURATION_MS },
        env: {
          type: "object",
          additionalProperties: { type: "string", maxLength: 2_048 },
          maxProperties: 16,
        },
      },
      required: ["executable", "args"],
      additionalProperties: false,
    },
    permissions: ["process.execute"],
    risk: "high",
    execute: (context: KernelToolExecutionContext) => execute(root, context),
  });
}
