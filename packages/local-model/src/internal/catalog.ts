import type { ModelPreset } from "./contracts.js";

const GIB = 1024 ** 3;

export const DEFAULT_PRESET = Object.freeze({
  id: "qwen3-1.7b-q4_k_m",
  displayName: "Qwen3 1.7B Q4_K_M",
  owner: "Qwen",
  repository: "Qwen3-1.7B-GGUF",
  revision: "7fb011e9aee6e4dc7adf8430df9ea8de6a466aa3",
  filename: "Qwen3-1.7B-Q4_K_M.gguf",
  sha256: "228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f",
  expectedDownloadBytes: 1_110_000_000,
  maxBytes: 2 * GIB,
  minimumMemoryBytes: 4 * GIB,
} as const satisfies ModelPreset);

export const MODEL_DOWNLOAD_URI =
  `https://huggingface.co/${DEFAULT_PRESET.owner}/${DEFAULT_PRESET.repository}` +
  `/resolve/${DEFAULT_PRESET.revision}/${DEFAULT_PRESET.filename}`;
