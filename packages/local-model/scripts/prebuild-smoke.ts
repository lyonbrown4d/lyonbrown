import { getLlama } from "node-llama-cpp";

if (process.platform !== "win32") {
  throw new Error("windows_prebuild_smoke_requires_windows");
}

const llama = await getLlama({
  gpu: false,
  build: "never",
  skipDownload: true,
  usePrebuiltBinaries: true,
  progressLogs: false,
  dryRun: true,
});

await llama.dispose();
process.stdout.write("official Windows CPU prebuild smoke passed\n");
