import { protocolPackageRoot, runBufWithZx } from "@lyonbrown/protocol/script/generation";

await runBufWithZx({
  args: ["lint"],
  cwd: protocolPackageRoot,
  timeoutMs: 30_000,
});
