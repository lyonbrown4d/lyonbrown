import type {
  DecisionModelBundleFileIdentity,
  DecisionModelBundleIdentity,
} from "@lyonbrown/decision-engine/internal/model-bundle-storage";

const REPOSITORY = "onnx-community/kev-0.6b-ONNX";

export const BUNDLED_DECISION_MODEL: DecisionModelBundleIdentity = Object.freeze({
  modelId: "kev-0.6b",
  revision: "5a827091b3d58fe4fdfc02a5a5b2ce364047d678",
  files: Object.freeze([
    Object.freeze({
      path: "added_tokens.json",
      sha256: "c0284b582e14987fbd3d5a2cb2bd139084371ed9acbae488829a1c900833c680",
      maxBytes: 707,
    }),
    Object.freeze({
      path: "config.json",
      sha256: "40ce1c875acf016ab7742ecdc45d3bad0fe8c51d3ead0ca2263ec5eb4e96f42d",
      maxBytes: 2_267,
    }),
    Object.freeze({
      path: "merges.txt",
      sha256: "8831e4f1a044471340f7c0a83d7bd71306a5b867e95fd870f74d0c5308a904d5",
      maxBytes: 1_671_853,
    }),
    Object.freeze({
      path: "onnx/model_q4.onnx",
      sha256: "db4657bb2f14f073cb4d289260dfc50af31adc1260a04cb2fe17c4f03da0881a",
      maxBytes: 1_245_327,
    }),
    Object.freeze({
      path: "onnx/model_q4.onnx_data",
      sha256: "20668ed757f5de294c254cab111910fbc37fb11addca8189e3f58b3c695f90f1",
      maxBytes: 374_822_912,
    }),
    Object.freeze({
      path: "special_tokens_map.json",
      sha256: "6676f091c8bc4d1b50146427cfde92073402866b87b6e39223227931b70083e9",
      maxBytes: 616,
    }),
    Object.freeze({
      path: "tokenizer.json",
      sha256: "c0382117ea329cdf097041132f6d735924b697924d6f6fc3945713e96ce87539",
      maxBytes: 7_031_645,
    }),
    Object.freeze({
      path: "tokenizer_config.json",
      sha256: "3c04ed3ca964ea2f6b2b5faf0dc4d31aec1cb1e8b4bcf63f402d295046b422b5",
      maxBytes: 9_678,
    }),
    Object.freeze({
      path: "vocab.json",
      sha256: "ca10d7e9fb3ed18575dd1e277a2579c16d108e32f27439684afa0e10b1440910",
      maxBytes: 2_776_833,
    }),
  ]),
});

export function bundledDecisionModelFileUrl(file: DecisionModelBundleFileIdentity): string {
  return `https://huggingface.co/${REPOSITORY}/resolve/${BUNDLED_DECISION_MODEL.revision}/${file.path}`;
}
