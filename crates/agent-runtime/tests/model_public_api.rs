use agent_runtime::model_manager::{
  DownloadPhase, ModelDownloadProgress, ModelPreset, ensure_model_with_progress,
};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

fn preset(bytes: &[u8]) -> ModelPreset {
  ModelPreset {
    id: "public-api-model",
    owner: "test-owner",
    repository: "test-repo",
    revision: "main",
    filename: "test.gguf",
    sha256: format!("{:x}", Sha256::digest(bytes)).into(),
    download_bytes: bytes.len() as u64,
    max_bytes: 1024,
    min_available_memory_bytes: 1,
  }
}

#[tokio::test]
async fn production_progress_api_uses_valid_cache_without_network() {
  let root = tempfile::tempdir().unwrap();
  let bytes = b"cached-production-api";
  let model = preset(bytes);
  std::fs::write(root.path().join(model.filename), bytes).unwrap();
  let (progress, observed) = tokio::sync::watch::channel(ModelDownloadProgress::new(
    DownloadPhase::WaitingForLock,
    0,
    Some(model.download_bytes),
  ));

  let path = ensure_model_with_progress(root.path(), &model, progress, CancellationToken::new())
    .await
    .unwrap();

  assert_eq!(std::fs::read(path).unwrap(), bytes);
  assert_eq!(observed.borrow().phase, DownloadPhase::Ready);
}
