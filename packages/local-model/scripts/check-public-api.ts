import { readFile } from "node:fs/promises";

const declarationFiles = ["dist/index.d.ts", "dist/public-model.d.ts", "dist/types.d.ts"];
const forbidden = /\b(filePath|modelPath|directoryPath|modelUri|url|Native|node-llama-cpp)\b/i;

for (const file of declarationFiles) {
  const declaration = await readFile(new URL(`../${file}`, import.meta.url), "utf8");
  const leaked = forbidden.exec(declaration)?.[0];
  if (leaked !== undefined) {
    throw new Error(`root public declaration leaks composition detail: ${leaked}`);
  }
}
