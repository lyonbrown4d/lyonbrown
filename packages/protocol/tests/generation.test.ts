import { existsSync, mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { checkGenerated } from "../scripts/generated-check.js";
import {
  BufCommandError,
  generate,
  type BufCommand,
  type BufCommandRunner,
} from "../scripts/generation.js";

const temporaryDirectories: string[] = [];

function temporaryDirectory(): string {
  const directory = mkdtempSync(join(tmpdir(), "lyonbrown-generation-test-"));
  temporaryDirectories.push(directory);
  return directory;
}

afterEach(async () => {
  const { rm } = await import("node:fs/promises");
  await Promise.all(
    temporaryDirectories
      .splice(0)
      .map((directory) => rm(directory, { force: true, recursive: true })),
  );
});

describe("Buf generation", () => {
  it("passes parameterized arguments and a bounded timeout to the runner", async () => {
    const invocations: BufCommand[] = [];
    const runner: BufCommandRunner = (command) => {
      invocations.push(command);
      return Promise.resolve();
    };
    const outputBase = temporaryDirectory();

    await generate(outputBase, { runBuf: runner, timeoutMs: 1_234 });

    expect(invocations).toHaveLength(1);
    expect(invocations[0]).toMatchObject({
      args: ["generate", "--template", "buf.gen.yaml", "--output", outputBase],
      timeoutMs: 1_234,
    });
  });

  it("maps process failures to a stable generation error", async () => {
    const runner: BufCommandRunner = () => Promise.reject(new Error("simulated process failure"));

    await expect(generate(temporaryDirectory(), { runBuf: runner })).rejects.toMatchObject({
      code: "BUF_COMMAND_FAILED",
    } satisfies Partial<BufCommandError>);
  });
});

describe("generated source checks", () => {
  it("always cleans its temporary directory when generation fails", async () => {
    let capturedTemporaryRoot: string | undefined;

    await expect(
      checkGenerated({
        generateSources: (outputBase) => {
          capturedTemporaryRoot = outputBase;
          mkdirSync(join(outputBase, "generated"), { recursive: true });
          writeFileSync(join(outputBase, "generated", "partial.ts"), "partial");
          return Promise.reject(
            new BufCommandError("BUF_COMMAND_FAILED", "simulated process failure"),
          );
        },
      }),
    ).rejects.toMatchObject({ code: "BUF_COMMAND_FAILED" });

    expect(capturedTemporaryRoot).toBeDefined();
    if (capturedTemporaryRoot === undefined) {
      expect.fail("generation must receive a temporary output root");
    }
    expect(existsSync(capturedTemporaryRoot)).toBe(false);
  });

  it("cleans its temporary directory after detecting stale output", async () => {
    const committed = temporaryDirectory();
    mkdirSync(join(committed, "agent"), { recursive: true });
    writeFileSync(join(committed, "agent", "contract.ts"), "old\n");
    let capturedTemporaryRoot: string | undefined;

    await expect(
      checkGenerated({
        committedDirectory: committed,
        generateSources: (outputBase) => {
          capturedTemporaryRoot = outputBase;
          mkdirSync(join(outputBase, "generated", "agent"), { recursive: true });
          writeFileSync(join(outputBase, "generated", "agent", "contract.ts"), "new\n");
          return Promise.resolve();
        },
      }),
    ).rejects.toThrow("generated protocol source is stale");

    expect(capturedTemporaryRoot).toBeDefined();
    if (capturedTemporaryRoot === undefined) {
      expect.fail("generation must receive a temporary output root");
    }
    expect(existsSync(capturedTemporaryRoot)).toBe(false);
  });
});
