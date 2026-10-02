use std::time::Duration;

use agent_runtime::model_manager::{
  DownloadPhase, ModelDownloadProgress, ModelPreset, default_preset,
  ensure_model_from_endpoint_with_progress,
};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_preset(bytes: &[u8], max_bytes: u64) -> ModelPreset {
  ModelPreset {
    id: "test-model",
    owner: "test-owner",
    repository: "test-repo",
    revision: "main",
    filename: "test.gguf",
    sha256: format!("{:x}", Sha256::digest(bytes)).into(),
    download_bytes: bytes.len() as u64,
    max_bytes,
    min_available_memory_bytes: 1,
  }
}

fn progress_channel(
  preset: &ModelPreset,
) -> (
  watch::Sender<ModelDownloadProgress>,
  watch::Receiver<ModelDownloadProgress>,
) {
  watch::channel(ModelDownloadProgress::new(
    DownloadPhase::Checking,
    0,
    Some(preset.download_bytes),
  ))
}

async fn mount_model(server: &MockServer, bytes: Vec<u8>, delay: Duration) {
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200).insert_header("content-length", bytes.len().to_string()),
    )
    .mount(server)
    .await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200)
        .set_delay(delay)
        .set_body_bytes(bytes),
    )
    .mount(server)
    .await;
}

async fn wait_for_phase(
  observed: &mut watch::Receiver<ModelDownloadProgress>,
  phase: DownloadPhase,
) {
  tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      if observed.borrow().phase == phase {
        return;
      }
      observed.changed().await.unwrap();
    }
  })
  .await
  .expect("phase was not published");
}

#[test]
fn default_download_is_the_small_balanced_model() {
  assert_eq!(default_preset().id, "qwen3-1.7b-q4_k_m");
}

#[tokio::test]
async fn progress_contains_waiting_for_lock_and_installing() {
  let server = MockServer::start().await;
  let bytes = b"GGUF-progress".to_vec();
  mount_model(&server, bytes.clone(), Duration::ZERO).await;
  let root = tempfile::tempdir().unwrap();
  let preset = test_preset(&bytes, 1024);
  let (progress, mut observed) = progress_channel(&preset);
  let endpoint = server.uri();
  let cancellation = CancellationToken::new();

  let download = tokio::spawn(async move {
    ensure_model_from_endpoint_with_progress(
      root.path(),
      &preset,
      &endpoint,
      progress,
      cancellation,
    )
    .await
  });
  wait_for_phase(&mut observed, DownloadPhase::WaitingForLock).await;
  wait_for_phase(&mut observed, DownloadPhase::Installing).await;

  download.await.unwrap().unwrap();
  assert_eq!(observed.borrow().phase, DownloadPhase::Ready);
  assert_eq!(observed.borrow().downloaded_bytes, bytes.len() as u64);
  assert_eq!(observed.borrow().total_bytes, Some(bytes.len() as u64));
}

#[tokio::test]
async fn progress_observes_nonzero_downloading_bytes() {
  let server = MockServer::start().await;
  let bytes = vec![b'p'; 2 * 1024 * 1024];
  mount_model(&server, bytes.clone(), Duration::ZERO).await;
  let root = tempfile::tempdir().unwrap();
  let preset = test_preset(&bytes, 4 * 1024 * 1024);
  let (progress, mut observed) = progress_channel(&preset);
  let endpoint = server.uri();

  let download = tokio::spawn(async move {
    ensure_model_from_endpoint_with_progress(
      root.path(),
      &preset,
      &endpoint,
      progress,
      CancellationToken::new(),
    )
    .await
  });
  tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      observed.changed().await.unwrap();
      let update = observed.borrow().clone();
      if update.phase == DownloadPhase::Downloading && update.downloaded_bytes > 0 {
        break;
      }
    }
  })
  .await
  .expect("nonzero download progress was not observable");

  download.await.unwrap().unwrap();
  assert_eq!(observed.borrow().phase, DownloadPhase::Ready);
  assert_eq!(observed.borrow().downloaded_bytes, bytes.len() as u64);
  assert_eq!(observed.borrow().total_bytes, Some(bytes.len() as u64));
}

#[tokio::test]
async fn cancellation_after_installing_still_finishes_ready() {
  let server = MockServer::start().await;
  let bytes = vec![b'i'; 1024 * 1024];
  mount_model(&server, bytes.clone(), Duration::ZERO).await;
  let root = tempfile::tempdir().unwrap();
  let root_path = root.path().to_path_buf();
  let preset = test_preset(&bytes, 2 * 1024 * 1024);
  let (progress, mut observed) = progress_channel(&preset);
  let cancellation = CancellationToken::new();
  let task_cancellation = cancellation.clone();
  let endpoint = server.uri();

  let download = tokio::spawn(async move {
    ensure_model_from_endpoint_with_progress(
      &root_path,
      &preset,
      &endpoint,
      progress,
      task_cancellation,
    )
    .await
  });
  wait_for_phase(&mut observed, DownloadPhase::Installing).await;
  cancellation.cancel();

  let path = download.await.unwrap().unwrap();
  assert_eq!(std::fs::read(&path).unwrap(), bytes);
  assert_eq!(observed.borrow().phase, DownloadPhase::Ready);
  assert_eq!(observed.borrow().downloaded_bytes, bytes.len() as u64);
  assert_eq!(observed.borrow().total_bytes, Some(bytes.len() as u64));
}

#[tokio::test]
async fn cancellation_while_waiting_for_lock_returns_without_network() {
  let server = MockServer::start().await;
  let bytes = vec![b'w'; 64 * 1024];
  mount_model(&server, bytes.clone(), Duration::from_secs(10)).await;
  let root = tempfile::tempdir().unwrap();
  let preset = test_preset(&bytes, 128 * 1024);
  let (first_progress, mut first_observed) = progress_channel(&preset);
  let first_cancel = CancellationToken::new();
  let first_cancel_task = first_cancel.clone();
  let first_endpoint = server.uri();
  let first_root = root.path().to_path_buf();
  let first_preset = preset.clone();
  let first = tokio::spawn(async move {
    ensure_model_from_endpoint_with_progress(
      &first_root,
      &first_preset,
      &first_endpoint,
      first_progress,
      first_cancel_task,
    )
    .await
  });
  wait_for_phase(&mut first_observed, DownloadPhase::Checking).await;

  let (second_progress, mut second_observed) = progress_channel(&preset);
  let second_cancel = CancellationToken::new();
  let second_cancel_task = second_cancel.clone();
  let second_endpoint = server.uri();
  let second_root = root.path().to_path_buf();
  let second = tokio::spawn(async move {
    ensure_model_from_endpoint_with_progress(
      &second_root,
      &preset,
      &second_endpoint,
      second_progress,
      second_cancel_task,
    )
    .await
  });
  wait_for_phase(&mut second_observed, DownloadPhase::WaitingForLock).await;
  second_cancel.cancel();

  let error = second.await.unwrap().unwrap_err();
  assert!(error.to_string().contains("cancelled"));
  first_cancel.cancel();
  assert!(first.await.unwrap().is_err());
}

#[tokio::test]
async fn cancellation_during_verification_cleans_staging() {
  let server = MockServer::start().await;
  let bytes = vec![b'v'; 16 * 1024 * 1024];
  mount_model(&server, bytes.clone(), Duration::ZERO).await;
  let root = tempfile::tempdir().unwrap();
  let root_path = root.path().to_path_buf();
  let preset = test_preset(&bytes, 32 * 1024 * 1024);
  let (progress, mut observed) = progress_channel(&preset);
  let cancellation = CancellationToken::new();
  let cancellation_task = cancellation.clone();
  let endpoint = server.uri();

  let download = tokio::spawn(async move {
    ensure_model_from_endpoint_with_progress(
      &root_path,
      &preset,
      &endpoint,
      progress,
      cancellation_task,
    )
    .await
  });
  wait_for_phase(&mut observed, DownloadPhase::Verifying).await;
  cancellation.cancel();

  let error = download.await.unwrap().unwrap_err();
  assert!(error.to_string().contains("cancelled"));
  assert_no_download_artifacts(root.path());
}

#[tokio::test]
async fn pre_cancelled_download_cleans_staging() {
  let root = tempfile::tempdir().unwrap();
  let preset = test_preset(b"not-downloaded", 1024);
  let (progress, _observed) = progress_channel(&preset);
  let cancellation = CancellationToken::new();
  cancellation.cancel();

  let error = ensure_model_from_endpoint_with_progress(
    root.path(),
    &preset,
    "http://127.0.0.1:9",
    progress,
    cancellation,
  )
  .await
  .unwrap_err();

  assert!(error.to_string().contains("cancelled"));
  assert_no_download_artifacts(root.path());
}

fn assert_no_download_artifacts(root: &std::path::Path) {
  let unexpected = std::fs::read_dir(root)
    .unwrap()
    .filter_map(Result::ok)
    .filter(|entry| entry.file_name() != ".locks")
    .collect::<Vec<_>>();
  assert!(unexpected.is_empty(), "unexpected cache artifacts");
}
