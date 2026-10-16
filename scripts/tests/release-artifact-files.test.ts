import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { assertPortableArtifactFiles } from "@lyonbrown/workspace/internal/release-artifact-files";

describe.skipIf(process.platform === "win32")("POSIX artifact link boundaries", () => {
  it("accepts relative file links inside the artifact while preserving Windows rejection", async () => {
    const scratch = await mkdtemp(join(tmpdir(), "lyonbrown-artifact-links-"));
    try {
      await mkdir(join(scratch, ".bin"));
      await writeFile(join(scratch, "cli.js"), "fixture");
      await symlink("../cli.js", join(scratch, ".bin", "cli"));
      await expect(assertPortableArtifactFiles(scratch, "darwin")).resolves.toBeUndefined();
      await expect(assertPortableArtifactFiles(scratch, "linux")).resolves.toBeUndefined();
      await expect(assertPortableArtifactFiles(scratch, "win32")).rejects.toThrow(
        "filesystem link",
      );
    } finally {
      await rm(scratch, { recursive: true, force: true });
    }
  });

  it.each(["absolute", "outside", "outside-return", "directory"])(
    "rejects %s links",
    async (kind) => {
      const scratch = await mkdtemp(join(tmpdir(), "lyonbrown-artifact-boundary-"));
      try {
        const artifact = join(scratch, "artifact");
        await mkdir(artifact);
        await writeFile(join(scratch, "outside"), "outside");
        await writeFile(join(artifact, "inside"), "inside");
        await symlink("artifact/inside", join(scratch, "outside-link"));
        await mkdir(join(artifact, "folder"));
        const target =
          kind === "absolute"
            ? join(artifact, "inside")
            : kind === "outside"
              ? "../outside"
              : kind === "outside-return"
                ? "../outside-link"
                : "folder";
        await symlink(target, join(artifact, "link"));
        await expect(assertPortableArtifactFiles(artifact, "darwin")).rejects.toThrow();
      } finally {
        await rm(scratch, { recursive: true, force: true });
      }
    },
  );
});
