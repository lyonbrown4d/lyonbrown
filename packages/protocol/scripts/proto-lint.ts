import { protocolPackageRoot, runBufWithZx } from "./generation.ts";

await runBufWithZx({
  args: ["lint"],
  cwd: protocolPackageRoot,
  timeoutMs: 30_000,
});
