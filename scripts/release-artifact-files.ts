import { lstat, readdir, readlink, realpath } from "node:fs/promises";
import { isAbsolute, join, relative, resolve, sep } from "node:path";

/** Windows forbids links; POSIX permits only self-contained relative file links. */
export async function assertPortableArtifactFiles(
  directory: string,
  platform: NodeJS.Platform = process.platform,
): Promise<void> {
  const root = await realpath(directory);
  const pending = [root];
  // Directory metadata has no batched fs API; cap concurrent directory reads.
  while (pending.length > 0) {
    const paths = pending.splice(0, 8);
    const batches = await Promise.all(paths.map((path) => readdir(path, { withFileTypes: true })));
    for (const [index, entries] of batches.entries()) {
      const parent = paths[index];
      if (parent === undefined) throw new Error("Invalid artifact traversal");
      for (const entry of entries) {
        const path = join(parent, entry.name);
        if (entry.isDirectory()) pending.push(path);
        if (!entry.isSymbolicLink()) continue;
        if (platform === "win32") throw new Error("Release artifact contains a filesystem link");
        const [link, target] = await Promise.all([readlink(path), realpath(path)]);
        const boundary = relative(root, target);
        const linkBoundary = relative(root, resolve(parent, link));
        if (
          isAbsolute(link) ||
          linkBoundary === ".." ||
          linkBoundary.startsWith(`..${sep}`) ||
          isAbsolute(linkBoundary) ||
          boundary === "" ||
          boundary === ".." ||
          boundary.startsWith(`..${sep}`) ||
          isAbsolute(boundary) ||
          !(await lstat(target)).isFile()
        ) {
          throw new Error("Release artifact contains a nonportable filesystem link");
        }
      }
    }
  }
}
