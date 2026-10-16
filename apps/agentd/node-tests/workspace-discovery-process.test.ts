import { mkdtemp, mkdir, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { createWorkspaceToolRegistry } from "@lyonbrown/agentd/internal/workspace-tools";

function parsed(content: string | undefined): Record<string, unknown> {
  return JSON.parse(content ?? "{}") as Record<string, unknown>;
}

describe("workspace discovery and process tools", () => {
  const directories: string[] = [];

  afterEach(async () => {
    await Promise.all(directories.splice(0).map((path) => rm(path, { recursive: true })));
  });

  async function fixture(): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-discovery-"));
    directories.push(root);
    return root;
  }

  it("lists and searches bounded workspace files while excluding escape links", async () => {
    const root = await fixture();
    const outside = await fixture();
    await mkdir(join(root, "nested"));
    await writeFile(join(root, "nested", "hello.txt"), "before\nneedle here\nafter", "utf8");
    await writeFile(join(outside, "secret.txt"), "needle outside", "utf8");
    await symlink(outside, join(root, "escape"), process.platform === "win32" ? "junction" : "dir");
    const registry = await createWorkspaceToolRegistry(root);
    const signal = new AbortController().signal;

    const listing = await registry.get("list_directory")?.execute({
      callId: "list",
      arguments: {},
      signal,
    });
    expect(parsed(listing?.content)).toMatchObject({
      entries: [{ path: "nested", kind: "directory" }],
      truncated: false,
    });
    const search = await registry.get("search_files")?.execute({
      callId: "search",
      arguments: { query: "needle" },
      signal,
    });
    expect(parsed(search?.content)).toEqual({
      matches: [{ path: "nested/hello.txt", line: 2, text: "needle here" }],
      truncated: false,
      skippedLarge: 0,
      skippedUnreadable: 0,
    });
    await expect(
      registry.get("list_directory")?.execute({
        callId: "escape",
        arguments: { path: "../" },
        signal,
      }),
    ).rejects.toThrow("workspace path denied");
  });

  it("executes literal Unicode arguments without shell expansion from a bounded cwd", async () => {
    const root = await fixture();
    const registry = await createWorkspaceToolRegistry(root);
    const result = await registry.get("process_execute")?.execute({
      callId: "argv",
      arguments: {
        executable: process.execPath,
        args: [
          "-e",
          "process.stdout.write(JSON.stringify(process.argv.slice(1)))",
          "space value",
          "雪 * $HOME",
        ],
      },
      signal: new AbortController().signal,
    });
    const output = parsed(result?.content);
    expect(output.exitCode).toBe(0);
    expect(typeof output.stdout).toBe("string");
    expect(JSON.parse(String(output.stdout)) as unknown).toEqual(["space value", "雪 * $HOME"]);
    expect(output.reason).toBeNull();
    await expect(
      registry.get("process_execute")?.execute({
        callId: "outside-cwd",
        arguments: { executable: process.execPath, args: ["-v"], cwd: "../" },
        signal: new AbortController().signal,
      }),
    ).rejects.toThrow("workspace path denied");
  });

  it("passes empty argv and explicit safe environment values without inheriting unrelated variables", async () => {
    const root = await fixture();
    const registry = await createWorkspaceToolRegistry(root);
    const result = await registry.get("process_execute")?.execute({
      callId: "environment",
      arguments: {
        executable: process.execPath,
        args: [
          "-e",
          "process.stdout.write(JSON.stringify({args:process.argv.slice(1),flag:process.env.SAFE_FLAG,path:typeof process.env.PATH}))",
          "",
        ],
        env: { SAFE_FLAG: "" },
      },
      signal: new AbortController().signal,
    });
    const output = parsed(result?.content);
    expect(JSON.parse(String(output.stdout)) as unknown).toEqual({
      args: [""],
      flag: "",
      path: "string",
    });
  });

  it("rejects unsafe process arguments, working directories, and environment", async () => {
    const root = await fixture();
    await writeFile(join(root, "file.txt"), "not a directory", "utf8");
    const registry = await createWorkspaceToolRegistry(root);
    const tool = registry.get("process_execute");
    const signal = new AbortController().signal;
    const run = (arguments_: Readonly<Record<string, unknown>>) =>
      tool?.execute({ callId: "invalid", arguments: arguments_, signal });

    await expect(run({ executable: "", args: [] })).rejects.toThrow("executable invalid");
    await expect(run({ executable: process.execPath, args: "-v" })).rejects.toThrow(
      "arguments invalid",
    );
    await expect(run({ executable: process.execPath, args: ["bad\0arg"] })).rejects.toThrow(
      "argument invalid",
    );
    await expect(run({ executable: process.execPath, args: [], cwd: "file.txt" })).rejects.toThrow(
      "working directory invalid",
    );
    await expect(run({ executable: process.execPath, args: [], timeoutMs: 0 })).rejects.toThrow(
      "timeout invalid",
    );
    await expect(run({ executable: process.execPath, args: [], env: ["bad"] })).rejects.toThrow(
      "process environment invalid",
    );
    await expect(
      run({ executable: process.execPath, args: [], env: { API_KEY: "secret" } }),
    ).rejects.toThrow("process environment key denied");
    await expect(
      run({ executable: process.execPath, args: [], env: { SAFE_FLAG: "x".repeat(2_049) } }),
    ).rejects.toThrow("process environment value invalid");
  });

  it("sanitizes launch errors and honors cancellation before spawn", async () => {
    const root = await fixture();
    const registry = await createWorkspaceToolRegistry(root);
    await expect(
      registry.get("process_execute")?.execute({
        callId: "missing-executable",
        arguments: { executable: join(root, "missing-program"), args: [] },
        signal: new AbortController().signal,
      }),
    ).rejects.toThrow("process could not start");

    const controller = new AbortController();
    controller.abort();
    await expect(
      registry.get("process_execute")?.execute({
        callId: "pre-cancelled",
        arguments: { executable: process.execPath, args: ["-v"] },
        signal: controller.signal,
      }),
    ).rejects.toThrow();
  });

  it("reports directory and search truncation at caller limits", async () => {
    const root = await fixture();
    await writeFile(join(root, "a.txt"), "needle one", "utf8");
    await writeFile(join(root, "b.txt"), "needle two", "utf8");
    const registry = await createWorkspaceToolRegistry(root);
    const signal = new AbortController().signal;
    const listed = await registry.get("list_directory")?.execute({
      callId: "limited-list",
      arguments: { limit: 1 },
      signal,
    });
    expect(parsed(listed?.content)).toMatchObject({
      entries: [{ path: "a.txt" }],
      truncated: true,
    });
    const searched = await registry.get("search_files")?.execute({
      callId: "limited-search",
      arguments: { query: "needle", limit: 1 },
      signal,
    });
    expect(parsed(searched?.content)).toMatchObject({
      matches: [{ path: "a.txt", line: 1 }],
      truncated: true,
    });
  });

  it("bounds output and stops timed out processes", async () => {
    const root = await fixture();
    const registry = await createWorkspaceToolRegistry(root);
    const output = await registry.get("process_execute")?.execute({
      callId: "large-output",
      arguments: {
        executable: process.execPath,
        args: ["-e", "process.stdout.write('x'.repeat(100000))"],
      },
      signal: new AbortController().signal,
    });
    const limited = parsed(output?.content);
    expect(String(limited.stdout).length).toBeLessThanOrEqual(4 * 1024);
    expect(limited.stdoutTruncated).toBe(true);

    const errorOutput = await registry.get("process_execute")?.execute({
      callId: "large-error-output",
      arguments: {
        executable: process.execPath,
        args: ["-e", "process.stderr.write('e'.repeat(100000))"],
      },
      signal: new AbortController().signal,
    });
    const limitedError = parsed(errorOutput?.content);
    expect(String(limitedError.stderr).length).toBeLessThanOrEqual(4 * 1024);
    expect(limitedError.stderrTruncated).toBe(true);

    const timeout = await registry.get("process_execute")?.execute({
      callId: "timeout",
      arguments: {
        executable: process.execPath,
        args: ["-e", "setInterval(() => {}, 1000)"],
        timeoutMs: 50,
      },
      signal: new AbortController().signal,
    });
    expect(parsed(timeout?.content).reason).toBe("timeout");
  });

  it("cancels an active process and excludes inherited daemon secrets", async () => {
    const root = await fixture();
    const registry = await createWorkspaceToolRegistry(root);
    const controller = new AbortController();
    const previous = process.env.LYONBROWN_DAEMON_TEST_SECRET;
    process.env.LYONBROWN_DAEMON_TEST_SECRET = "hidden";
    try {
      const promise = registry.get("process_execute")?.execute({
        callId: "cancel",
        arguments: {
          executable: process.execPath,
          args: [
            "-e",
            "process.stdout.write(String(process.env.LYONBROWN_DAEMON_TEST_SECRET)); setInterval(() => {}, 1000)",
          ],
          timeoutMs: 10_000,
        },
        signal: controller.signal,
      });
      setTimeout(() => controller.abort(), 200);
      const result = parsed((await promise)?.content);
      expect(result.reason).toBe("cancelled");
      expect(result.stdout).toBe("undefined");
    } finally {
      if (previous === undefined) delete process.env.LYONBROWN_DAEMON_TEST_SECRET;
      else process.env.LYONBROWN_DAEMON_TEST_SECRET = previous;
    }
  });

  it.runIf(process.platform !== "win32")("kills descendants on timeout", async () => {
    const root = await fixture();
    const registry = await createWorkspaceToolRegistry(root);
    const script = [
      "const {spawn} = require('node:child_process');",
      "const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)']);",
      "process.stdout.write(String(child.pid));",
      "setInterval(() => {}, 1000);",
    ].join("");
    const result = await registry.get("process_execute")?.execute({
      callId: "tree-timeout",
      arguments: { executable: process.execPath, args: ["-e", script], timeoutMs: 500 },
      signal: new AbortController().signal,
    });
    const output = parsed(result?.content);
    expect(output.reason).toBe("timeout");
    const descendantPid = Number(output.stdout);
    expect(Number.isSafeInteger(descendantPid)).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 100));
    if (process.platform === "linux") {
      const status = await readFile(`/proc/${String(descendantPid)}/stat`, "utf8").catch(() => "");
      expect(status === "" || status.includes(") Z ")).toBe(true);
    } else {
      expect(() => process.kill(descendantPid, 0)).toThrow();
    }
  });
});
