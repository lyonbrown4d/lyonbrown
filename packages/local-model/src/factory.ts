import { homedir } from "node:os";
import { join } from "node:path";

import { DEFAULT_PRESET } from "./internal/catalog.js";
import { createLocalModelManager } from "./internal/manager.js";
import { createNodeLlamaEngine } from "./internal/node-llama.js";
import { NodeModelIo } from "./internal/node-model-io.js";
import type { LocalModelService } from "./types.js";

export const createDefaultLocalModelService = (): LocalModelService => {
  const modelsDirectory = join(homedir(), ".lyonbrown", "models");
  const io = new NodeModelIo(modelsDirectory);
  return createLocalModelManager(
    io,
    createNodeLlamaEngine(join(modelsDirectory, DEFAULT_PRESET.filename)),
  );
};
