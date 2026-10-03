import { protocolPackageRoot, runBufWithZx } from "./generation.js";

await runBufWithZx({
  args: ["lint"],
  cwd: protocolPackageRoot,
  timeoutMs: 30_000,
});
