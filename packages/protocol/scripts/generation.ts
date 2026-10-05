import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { $, usePowerShell, type Options } from "zx";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_TIMEOUT_MS = 30_000;

if (process.platform === "win32") {
  usePowerShell();
}

const BUF_PLATFORM_PACKAGES: Readonly<Record<string, string>> = Object.freeze({
  "darwin-arm64": "@bufbuild/buf-darwin-arm64",
  "darwin-x64": "@bufbuild/buf-darwin-x64",
  "linux-arm": "@bufbuild/buf-linux-armv7",
  "linux-arm64": "@bufbuild/buf-linux-aarch64",
  "linux-x64": "@bufbuild/buf-linux-x64",
  "win32-arm64": "@bufbuild/buf-win32-arm64",
  "win32-x64": "@bufbuild/buf-win32-x64",
});

export interface BufCommand {
  readonly args: readonly string[];
  readonly cwd: string;
  readonly environment?: NodeJS.ProcessEnv;
  readonly timeoutMs: number;
}

export type BufCommandRunner = (command: BufCommand) => Promise<void>;

export type BufCommandErrorCode = "BUF_COMMAND_FAILED" | "BUF_PLATFORM_UNSUPPORTED";

export class BufCommandError extends Error {
  public override readonly name = "BufCommandError";

  public constructor(
    public readonly code: BufCommandErrorCode,
    message: string,
    options?: ErrorOptions,
  ) {
    super(message, options);
  }
}

function resolveBufBinary(): string {
  const platformKey = `${process.platform}-${process.arch}`;
  const platformPackage = BUF_PLATFORM_PACKAGES[platformKey];
  if (platformPackage === undefined) {
    throw new BufCommandError(
      "BUF_PLATFORM_UNSUPPORTED",
      `Buf does not publish a locked binary for ${platformKey}`,
    );
  }

  const require = createRequire(import.meta.url);
  const bufPackageJson = require.resolve("@bufbuild/buf/package.json");
  const requireFromBuf = createRequire(bufPackageJson);
  const executable = process.platform === "win32" ? "buf.exe" : "buf";
  return requireFromBuf.resolve(`${platformPackage}/bin/${executable}`);
}

export const runBufWithZx: BufCommandRunner = async (command) => {
  const executable = resolveBufBinary();
  const executionOptions: Partial<Options> = {
    cwd: command.cwd,
    quiet: true,
    timeout: command.timeoutMs,
  };
  if (command.environment !== undefined) {
    executionOptions.env = command.environment;
  }
  const executor = $(executionOptions);
  if (process.platform === "win32") {
    await executor`& ${executable} ${command.args}`;
  } else {
    await executor`${executable} ${command.args}`;
  }
};

export interface GenerateOptions {
  readonly runBuf?: BufCommandRunner;
  readonly timeoutMs?: number;
}

async function executeBuf(command: BufCommand, runBuf: BufCommandRunner): Promise<void> {
  try {
    await runBuf(command);
  } catch (cause: unknown) {
    if (cause instanceof BufCommandError) {
      throw cause;
    }
    throw new BufCommandError("BUF_COMMAND_FAILED", "locked Buf command failed", { cause });
  }
}

export async function generate(
  outputBaseDirectory: string,
  options: GenerateOptions = {},
): Promise<void> {
  await executeBuf(
    {
      args: ["generate", "--template", "buf.gen.yaml", "--output", outputBaseDirectory],
      cwd: packageRoot,
      timeoutMs: options.timeoutMs ?? DEFAULT_TIMEOUT_MS,
    },
    options.runBuf ?? runBufWithZx,
  );
}

export async function buildDescriptorSet(
  inputDirectory: string,
  outputFile: string,
  options: GenerateOptions = {},
): Promise<void> {
  await executeBuf(
    {
      args: [
        "build",
        inputDirectory,
        "--as-file-descriptor-set",
        "--exclude-source-info",
        "--output",
        outputFile,
      ],
      cwd: packageRoot,
      timeoutMs: options.timeoutMs ?? DEFAULT_TIMEOUT_MS,
    },
    options.runBuf ?? runBufWithZx,
  );
}

export const generatedOutputBase = join(packageRoot, "src");
export const protocolPackageRoot = packageRoot;
