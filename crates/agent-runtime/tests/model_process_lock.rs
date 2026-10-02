use std::process::{Command, Stdio};
use std::time::Duration;

use agent_runtime::model_manager::{
  DownloadPhase, ModelDownloadProgress, ModelPreset, ensure_model_from_endpoint_with_progress,
};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CACHE_ENV: &str = "LYONBROWN_TEST_MODEL_CACHE";
const ENDPOINT_ENV: &str = "LYONBROWN_TEST_MODEL_ENDPOINT";
const MARKER_ENV: &str = "LYONBROWN_TEST_MODEL_MARKER";
const HOLDER_ENV: &str = "LYONBROWN_TEST_MODEL_LOCK_HOLDER";

fn preset(bytes: &[u8]) -> ModelPreset {
  ModelPreset {
    id: "process-model",
    owner: "test-owner",
    repository: "test-repo",
    revision: "main",
    filename: "shared.gguf",
    sha256: format!("{:x}", Sha256::digest(bytes)).into(),
    download_bytes: bytes.len() as u64,
    max_bytes: 1024,
    min_available_memory_bytes: 1,
  }
}

fn progress(
  model: &ModelPreset,
) -> (
  watch::Sender<ModelDownloadProgress>,
  watch::Receiver<ModelDownloadProgress>,
) {
  watch::channel(ModelDownloadProgress::new(
    DownloadPhase::WaitingForLock,
    0,
    Some(model.download_bytes),
  ))
}

async fn wait_for_marker(marker: &std::path::Path) {
  tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      if tokio::fs::try_exists(marker).await.unwrap() {
        return;
      }
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
  })
  .await
  .expect("holder did not acquire destination lock");
}

async fn mount_success(server: &MockServer, bytes: &[u8]) {
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/shared.gguf"))
    .respond_with(
      ResponseTemplate::new(200).insert_header("content-length", bytes.len().to_string()),
    )
    .mount(server)
    .await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/shared.gguf"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
    .mount(server)
    .await;
}

#[tokio::test]
#[ignore]
async fn hold_destination_lock_in_child() {
  if std::env::var_os(HOLDER_ENV).is_none() {
    return;
  }
  let cache = std::path::PathBuf::from(std::env::var_os(CACHE_ENV).unwrap());
  let endpoint = std::env::var(ENDPOINT_ENV).unwrap();
  let marker = std::path::PathBuf::from(std::env::var_os(MARKER_ENV).unwrap());
  let model = preset(b"holder-model");
  let (sender, mut observed) = progress(&model);
  tokio::spawn(async move {
    loop {
      observed.changed().await.unwrap();
      if observed.borrow().phase == DownloadPhase::Checking {
        std::fs::write(marker, b"locked").unwrap();
        return;
      }
    }
  });

  ensure_model_from_endpoint_with_progress(
    &cache,
    &model,
    &endpoint,
    sender,
    CancellationToken::new(),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn process_exit_releases_destination_lock_and_waiter_can_cancel() {
  let hold_server = MockServer::start().await;
  Mock::given(method("HEAD"))
    .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
    .mount(&hold_server)
    .await;
  let success_server = MockServer::start().await;
  let waiter_bytes = b"waiter-model";
  mount_success(&success_server, waiter_bytes).await;
  let root = tempfile::tempdir().unwrap();
  let marker = root.path().join("holder-ready");
  let mut child = Command::new(std::env::current_exe().unwrap())
    .arg("--ignored")
    .arg("--exact")
    .arg("hold_destination_lock_in_child")
    .arg("--nocapture")
    .env(HOLDER_ENV, "1")
    .env(CACHE_ENV, root.path())
    .env(ENDPOINT_ENV, hold_server.uri())
    .env(MARKER_ENV, &marker)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .unwrap();
  wait_for_marker(&marker).await;

  let model = preset(waiter_bytes);
  let (sender, _observed) = progress(&model);
  let cancellation = CancellationToken::new();
  let task_cancellation = cancellation.clone();
  let cache = root.path().to_path_buf();
  let endpoint = success_server.uri();
  let wait = tokio::spawn(async move {
    ensure_model_from_endpoint_with_progress(&cache, &model, &endpoint, sender, task_cancellation)
      .await
  });
  tokio::time::sleep(Duration::from_millis(250)).await;
  let was_waiting = !wait.is_finished();
  cancellation.cancel();
  let wait_result = tokio::time::timeout(Duration::from_secs(5), wait).await;
  child.kill().unwrap();
  child.wait().unwrap();

  assert!(was_waiting, "different digests bypassed destination lock");
  assert!(wait_result.unwrap().unwrap().is_err());
  let retry_model = preset(waiter_bytes);
  let (retry_progress, _) = progress(&retry_model);
  let path = tokio::time::timeout(
    Duration::from_secs(5),
    ensure_model_from_endpoint_with_progress(
      root.path(),
      &retry_model,
      &success_server.uri(),
      retry_progress,
      CancellationToken::new(),
    ),
  )
  .await
  .unwrap()
  .unwrap();
  assert_eq!(std::fs::read(path).unwrap(), waiter_bytes);
  assert!(
    std::fs::read_dir(root.path())
      .unwrap()
      .filter_map(Result::ok)
      .all(|entry| !entry
        .file_name()
        .to_string_lossy()
        .starts_with(".download-"))
  );
}
