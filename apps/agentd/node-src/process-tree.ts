import { spawn, type ChildProcess } from "node:child_process";
import { join } from "node:path";

export function terminateProcessTree(child: ChildProcess): void {
  const pid = child.pid;
  if (pid === undefined) return;
  if (process.platform === "win32") {
    const candidate = process.env.SystemRoot ?? process.env.SYSTEMROOT;
    const systemRoot = candidate && !candidate.includes("\0") ? candidate : "C:\\Windows";
    try {
      const killer = spawn(
        join(systemRoot, "System32", "taskkill.exe"),
        ["/PID", String(pid), "/T", "/F"],
        { stdio: "ignore", windowsHide: true },
      );
      killer.once("error", () => {
        child.kill();
      });
      killer.once("close", (code) => {
        if (code !== 0) child.kill();
      });
    } catch {
      child.kill();
    }
    return;
  }
  try {
    process.kill(-pid, "SIGKILL");
  } catch {
    child.kill("SIGKILL");
  }
}
