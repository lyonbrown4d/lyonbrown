#![cfg(feature = "semantic-local")]
use decision_core::BackendError;
use decision_semantic::{LocalFastEmbed, OfflineEmbeddingConfig, OfflinePooling, VerifiedAsset};

#[tokio::test]
async fn corrupt_assets_fail_before_loading_the_native_runtime() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("invalid-asset");
  std::fs::write(&path, b"invalid asset").expect("asset");
  let asset = VerifiedAsset {
    path,
    sha256: "00".repeat(32),
  };
  let config = OfflineEmbeddingConfig {
    model: asset.clone(),
    tokenizer: asset.clone(),
    config: asset.clone(),
    special_tokens: asset.clone(),
    tokenizer_config: asset.clone(),
    runtime_library: asset,
    model_revision: "fixture-v1".into(),
    preprocessing_revision: "fixture-v1".into(),
    license: "fixture".into(),
    dimensions: 3,
    max_tokens: 32,
    intra_threads: 1,
    max_asset_bytes: 1024,
    pooling: OfflinePooling::Mean,
  };
  assert!(matches!(
    LocalFastEmbed::load(config).await,
    Err(BackendError::InvalidInput)
  ));
}
