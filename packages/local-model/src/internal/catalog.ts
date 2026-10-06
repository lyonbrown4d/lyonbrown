import type { ModelPreset } from "@lyonbrown/local-model/internal/internal/contracts";

const GIB = 1024 ** 3;

export const MODEL_CATALOG = Object.freeze([
  Object.freeze({
    id: "qwen3-0.6b-q8_0",
    displayName: "Qwen3 0.6B Q8_0",
    owner: "Qwen",
    repository: "Qwen3-0.6B-GGUF",
    revision: "23749fefcc72300e3a2ad315e1317431b06b590a",
    filename: "Qwen3-0.6B-Q8_0.gguf",
    sha256: "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031",
    expectedDownloadBytes: 639_446_688,
    maxBytes: GIB,
    minimumMemoryBytes: 2 * GIB,
    minimumContextTokens: 4_096,
    qualityRank: 1,
    license: "apache-2.0",
  }),
  Object.freeze({
    id: "qwen3-1.7b-q4_k_m",
    displayName: "Qwen3 1.7B Q4_K_M",
    owner: "Qwen",
    repository: "Qwen3-1.7B-GGUF",
    revision: "7fb011e9aee6e4dc7adf8430df9ea8de6a466aa3",
    filename: "Qwen3-1.7B-Q4_K_M.gguf",
    sha256: "228fb5627f7510b8b3516cdb6435e4b0d2a2bf330fe5b0ab19284a3570a8bb1f",
    expectedDownloadBytes: 1_107_408_544,
    maxBytes: 2 * GIB,
    minimumMemoryBytes: 4 * GIB,
    minimumContextTokens: 4_096,
    qualityRank: 2,
    license: "apache-2.0",
  }),
  Object.freeze({
    id: "qwen3-4b-q4_k_m",
    displayName: "Qwen3 4B Q4_K_M",
    owner: "Qwen",
    repository: "Qwen3-4B-GGUF",
    revision: "bc640142c66e1fdd12af0bd68f40445458f3869b",
    filename: "Qwen3-4B-Q4_K_M.gguf",
    sha256: "7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5",
    expectedDownloadBytes: 2_497_280_256,
    maxBytes: 4 * GIB,
    minimumMemoryBytes: 8 * GIB,
    minimumContextTokens: 4_096,
    qualityRank: 3,
    license: "apache-2.0",
  }),
] as const satisfies readonly ModelPreset[]);

export const CATALOG_FALLBACK_PRESET: ModelPreset = MODEL_CATALOG[0];

/** Compatibility alias; runtime selection uses MODEL_CATALOG and a ModelSelector. */
export const DEFAULT_PRESET = CATALOG_FALLBACK_PRESET;

export const modelDownloadUri = (preset: ModelPreset): string =>
  `https://huggingface.co/${preset.owner}/${preset.repository}` +
  `/resolve/${preset.revision}/${preset.filename}`;

/** Compatibility URI for callers that still explicitly select the fallback preset. */
export const MODEL_DOWNLOAD_URI = modelDownloadUri(CATALOG_FALLBACK_PRESET);
