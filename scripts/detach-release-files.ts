import { randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { copyFile, lstat, readdir, rename, rm } from "node:fs/promises";
import { join } from "node:path";

/** pnpm workspace injection may retain source hard links even with copy imports. */
export async function detachReleaseFiles(directory: string): Promise<void> {
  const pending = [directory];
  while (pending.length > 0) {
    const directories = pending.splice(0, 8);
    const batches = await Promise.all(
      directories.map(async (parent) =>
        (await readdir(parent, { withFileTypes: true })).map((entry) => ({
          entry,
          path: join(parent, entry.name),
        })),
      ),
    );
    const files: string[] = [];
    for (const { entry, path } of batches.flat()) {
      if (entry.isDirectory()) pending.push(path);
      if (entry.isFile()) files.push(path);
    }
    for (let offset = 0; offset < files.length; offset += 32) {
      await Promise.all(files.slice(offset, offset + 32).map(detachFile));
    }
  }
}

async function detachFile(path: string): Promise<void> {
  if ((await lstat(path)).nlink <= 1) return;
  const temporary = `${path}.release-copy-${randomUUID()}`;
  try {
    await copyFile(path, temporary, constants.COPYFILE_EXCL | constants.COPYFILE_FICLONE);
    await rename(temporary, path);
  } finally {
    await rm(temporary, { force: true });
  }
}
