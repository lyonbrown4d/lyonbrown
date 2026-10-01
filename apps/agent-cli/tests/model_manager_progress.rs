use std::sync::atomic::{AtomicUsize, Ordering};

use agent_cli::model_manager::{
  DownloadPhase, ModelDownloadProgress, ModelPreset, cached_model, default_preset,
  ensure_model_from_endpoint_with_progress,
};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

fn temp_dir() -> std::path::PathBuf {
  let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
  let path = std::env::temp_dir().join(format!(
    "lyonbrown-model-progress-{}-{id}",
    std::process::id()
  ));
  std::fs::create_dir_all(&path).unwrap();
  path
}

fn test_preset(bytes: &[u8]) -> ModelPreset {
  ModelPreset {
    id: "test-model",
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
async fn cache_lookup_does_not_start_a_download() {
  let root = temp_dir();
  let preset = test_preset(b"not-downloaded");

  assert!(cached_model(&root, &preset).await.unwrap().is_none());
  assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
  std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn download_reports_progress_until_ready() {
  let server = MockServer::start().await;
  let bytes = b"GGUF-progress";
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200).insert_header("content-length", bytes.len().to_string()),
    )
    .mount(&server)
    .await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
    .mount(&server)
    .await;
  let root = temp_dir();
  let preset = test_preset(bytes);
  let (progress, observed) = tokio::sync::watch::channel(ModelDownloadProgress::new(
    DownloadPhase::Checking,
    0,
    Some(bytes.len() as u64),
  ));

  ensure_model_from_endpoint_with_progress(
    &root,
    &preset,
    &server.uri(),
    progress,
    tokio_util::sync::CancellationToken::new(),
  )
  .await
  .unwrap();

  assert_eq!(observed.borrow().phase, DownloadPhase::Ready);
  assert_eq!(observed.borrow().downloaded_bytes, bytes.len() as u64);
  std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn default_download_is_the_small_balanced_model() {
  assert_eq!(default_preset().id, "qwen3-1.7b-q4_k_m");
}

#[tokio::test]
async fn cancelled_download_cleans_staging_files() {
  let root = temp_dir();
  let preset = test_preset(b"not-downloaded");
  let (progress, _observed) = tokio::sync::watch::channel(ModelDownloadProgress::new(
    DownloadPhase::Checking,
    0,
    Some(preset.download_bytes),
  ));
  let cancellation = tokio_util::sync::CancellationToken::new();
  cancellation.cancel();

  let error = ensure_model_from_endpoint_with_progress(
    &root,
    &preset,
    "http://127.0.0.1:9",
    progress,
    cancellation,
  )
  .await
  .unwrap_err();

  assert!(error.to_string().contains("cancelled"));
  assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
  std::fs::remove_dir_all(root).unwrap();
}
