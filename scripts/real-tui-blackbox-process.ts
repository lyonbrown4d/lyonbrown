import { type ChildProcess } from "node:child_process";
import { rm } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

import {
  ControlPlaneError,
  type DiscoveryDescriptor,
  type SecureRunDirectory,
} from "@lyonbrown/local-control-plane";

export interface PtyExitSource {
  onExit(
    listener: (event: { readonly exitCode: number; readonly signal?: number }) => void,
  ): unknown;
}

export interface ProcessExit {
  readonly code: number | null;
  readonly signal: NodeJS.Signals | null;
}

export function requiredAbsoluteEnvironmentPath(
  environment: Readonly<NodeJS.ProcessEnv>,
  name: string,
): string {
  const value = environment[name]?.trim();
  if (value === undefined || value.length === 0 || !isAbsolute(value)) {
    throw new Error(`${name} must be an absolute path injected through dotenv or the environment`);
  }
  return resolve(value);
}

export function childEnvironment(
  source: Readonly<NodeJS.ProcessEnv>,
  homeDirectory: string,
): NodeJS.ProcessEnv {
  const environment = { ...source };
  environment.USERPROFILE = homeDirectory;
  environment.HOME = homeDirectory;
  environment.TERM = "xterm-256color";
  environment.COLORTERM = "truecolor";
  delete environment.NO_COLOR;
  return environment;
}

export async function waitForDiscovery(
  openDirectory: () => Promise<SecureRunDirectory>,
  daemon: ChildProcess,
  exited: Promise<ProcessExit>,
  timeoutMs: number,
  acceptDescriptor?: (descriptor: DiscoveryDescriptor) => boolean,
): Promise<DiscoveryDescriptor> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (daemon.exitCode !== null || daemon.signalCode !== null) {
      const result = await exited;
      throw new Error(
        `agentd exited before discovery (${String(result.code)}, ${String(result.signal)})`,
      );
    }
    let directory: SecureRunDirectory | undefined;
    try {
      directory = await openDirectory();
      const descriptor = await directory.discovery.read();
      if (acceptDescriptor === undefined || acceptDescriptor(descriptor)) return descriptor;
    } catch (error: unknown) {
      const pending =
        error instanceof ControlPlaneError &&
        (error.code === "DESCRIPTOR_NOT_FOUND" ||
          (error.code === "PLATFORM_SECURITY_FAILURE" &&
            error.details?.operation === "read_descriptor"));
      if (!pending) throw error;
    } finally {
      await directory?.close();
    }
    await delay(50);
  }
  throw new Error("agentd did not publish discovery before the startup deadline");
}

export function assertDescriptor(
  descriptor: DiscoveryDescriptor,
  daemon: ChildProcess,
  scopeId: string,
): void {
  if (daemon.pid === undefined || descriptor.pid !== daemon.pid) {
    throw new Error("discovery descriptor does not identify the independently started agentd");
  }
  if (descriptor.scopeId !== scopeId) throw new Error("discovery descriptor scope mismatch");
}

export function processExit(child: ChildProcess): Promise<ProcessExit> {
  return new Promise((resolveExit) => {
    child.once("exit", (code, signal) => {
      resolveExit({ code, signal });
    });
  });
}

export function ptyExit(
  terminal: PtyExitSource,
): Promise<{ readonly exitCode: number; readonly signal?: number }> {
  return new Promise((resolveExit) => {
    terminal.onExit(resolveExit);
  });
}

export async function waitUntil(
  predicate: () => boolean,
  timeoutMs: number,
  message: string,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await delay(25);
  }
  throw new Error(message);
}

export async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const timeout = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(() => reject(new Error(message)), timeoutMs);
  });
  try {
    return await Promise.race([promise, timeout]);
  } finally {
    clearTimeout(timer);
  }
}

export async function stopProcess(
  child: ChildProcess | undefined,
  timeoutMs: number,
): Promise<void> {
  if (child === undefined || child.pid === undefined) return;
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = processExit(child);
  child.kill("SIGTERM");
  try {
    await withTimeout(exited, timeoutMs, "agentd graceful shutdown timed out");
  } catch {
    child.kill("SIGKILL");
    await withTimeout(exited, timeoutMs, "agentd forced shutdown timed out");
  }
}

export async function removeOwnedRunDirectory(scratchRoot: string, runRoot: string): Promise<void> {
  const child = relative(scratchRoot, runRoot);
  if (child.length === 0 || child.startsWith("..") || isAbsolute(child)) {
    throw new Error("refusing to remove a black-box directory outside the configured scratch root");
  }
  await rm(runRoot, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
}

export function appendBounded(current: string, addition: string): string {
  return `${current}${addition}`.slice(-32_768);
}
