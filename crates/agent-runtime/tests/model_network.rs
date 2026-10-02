use std::time::Duration;

use agent_runtime::model_manager::{
  DownloadPhase, ModelDownloadProgress, ModelDownloadTimeouts, ModelPreset,
  ensure_model_from_endpoint_with_progress_and_timeouts,
};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn preset(bytes: &[u8]) -> ModelPreset {
  ModelPreset {
    id: "network-model",
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

fn progress(model: &ModelPreset) -> watch::Sender<ModelDownloadProgress> {
  watch::channel(ModelDownloadProgress::new(
    DownloadPhase::WaitingForLock,
    0,
    Some(model.download_bytes),
  ))
  .0
}

#[tokio::test]
async fn delayed_head_is_bounded_by_request_timeout() {
  let server = MockServer::start().await;
  let bytes = b"timeout";
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
    .mount(&server)
    .await;
  let root = tempfile::tempdir().unwrap();
  let model = preset(bytes);
  let timeouts = ModelDownloadTimeouts {
    connect: Duration::from_millis(50),
    request: Duration::from_millis(50),
    idle: Duration::from_millis(50),
  };

  let error = ensure_model_from_endpoint_with_progress_and_timeouts(
    root.path(),
    &model,
    &server.uri(),
    progress(&model),
    CancellationToken::new(),
    timeouts,
  )
  .await
  .unwrap_err();

  assert!(error.to_string().contains("timed out"));
}

#[tokio::test]
async fn local_redirect_chain_downloads_successfully() {
  let server = MockServer::start().await;
  let bytes = b"redirected-model";
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(302).insert_header("location", "/metadata-hop"))
    .mount(&server)
    .await;
  Mock::given(method("HEAD"))
    .and(path("/metadata-hop"))
    .respond_with(ResponseTemplate::new(307).insert_header("location", "/final-model"))
    .mount(&server)
    .await;
  Mock::given(method("HEAD"))
    .and(path("/final-model"))
    .respond_with(
      ResponseTemplate::new(200).insert_header("content-length", bytes.len().to_string()),
    )
    .mount(&server)
    .await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(302).insert_header("location", "/download-hop"))
    .mount(&server)
    .await;
  Mock::given(method("GET"))
    .and(path("/download-hop"))
    .respond_with(ResponseTemplate::new(307).insert_header("location", "/final-model"))
    .mount(&server)
    .await;
  Mock::given(method("GET"))
    .and(path("/final-model"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
    .mount(&server)
    .await;
  let root = tempfile::tempdir().unwrap();
  let model = preset(bytes);

  let path = ensure_model_from_endpoint_with_progress_and_timeouts(
    root.path(),
    &model,
    &server.uri(),
    progress(&model),
    CancellationToken::new(),
    ModelDownloadTimeouts::default(),
  )
  .await
  .unwrap();

  assert_eq!(std::fs::read(path).unwrap(), bytes);
}
