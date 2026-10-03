import { rm } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const packageRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
const distPath = resolve(packageRoot, "dist");

export async function cleanDist(): Promise<void> {
  if (dirname(distPath) !== packageRoot || basename(distPath) !== "dist") {
    throw new Error("refusing to clean outside the package dist directory");
  }
  await rm(distPath, { force: true, recursive: true });
}

const entryPath = process.argv[1];
if (entryPath && import.meta.url === pathToFileURL(resolve(entryPath)).href) {
  await cleanDist();
}
