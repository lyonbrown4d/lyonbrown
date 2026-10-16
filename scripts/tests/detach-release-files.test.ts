import { link, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it } from "vitest";

import { detachReleaseFiles } from "@lyonbrown/workspace/internal/detach-release-files";

it("keeps a deployed artifact unchanged after its hard-linked source is rebuilt", async () => {
  const scratch = await mkdtemp(join(tmpdir(), "lyonbrown-release-copy-"));
  try {
    const source = join(scratch, "source.js");
    const artifact = join(scratch, "artifact");
    await mkdir(join(artifact, "dist"), { recursive: true });
    const deployed = join(artifact, "dist", "main.js");
    await writeFile(source, "built", { mode: 0o755 });
    await link(source, deployed);
    expect((await stat(deployed)).nlink).toBe(2);
    await detachReleaseFiles(artifact);
    expect((await stat(deployed)).nlink).toBe(1);
    await writeFile(source, "next build");
    expect(await readFile(deployed, "utf8")).toBe("built");
    if (process.platform !== "win32") expect((await stat(deployed)).mode & 0o777).toBe(0o755);
    await expect(detachReleaseFiles(artifact)).resolves.toBeUndefined();
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
});
