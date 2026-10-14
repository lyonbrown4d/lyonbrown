import { spawn, type ChildProcess } from "node:child_process";
import { resolve } from "node:path";
import {
  runtimeConfiguration,
  workerEnvironment,
} from "@lyonbrown/agent-desktop/internal/main/environment";
export function spawnClientWorker(): ChildProcess {
  const configuration = runtimeConfiguration();
  return spawn(configuration.nodeExecutable, [resolve(import.meta.dirname, "client-worker.js")], {
    cwd: configuration.workspace,
    env: workerEnvironment(),
    windowsHide: true,
    stdio: ["ignore", "ignore", "ignore", "ipc"],
  });
}
