use std::path::Path;

use agent_runtime::model_manager::{
  DownloadPhase, ModelDownloadProgress, ModelPreset, ensure_model_from_endpoint,
  finalize_download_after_cleanup, model_destination_key,
};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn preset(bytes: &[u8], max_bytes: u64) -> ModelPreset {
  ModelPreset {
    id: "atomic-model",
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

async fn mount_model(server: &MockServer, bytes: &[u8]) {
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200).insert_header("content-length", bytes.len().to_string()),
    )
    .mount(server)
    .await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
    .mount(server)
    .await;
}

fn assert_no_staging(root: &Path) {
  let staging = std::fs::read_dir(root)
    .unwrap()
    .filter_map(Result::ok)
    .filter(|entry| {
      entry
        .file_name()
        .to_string_lossy()
        .starts_with(".download-")
    })
    .collect::<Vec<_>>();
  assert!(staging.is_empty(), "staging artifacts remain");
}

#[test]
fn tempfile_persist_can_replace_a_file_in_cache_directory() {
  let root = tempfile::tempdir().unwrap();
  let source = root.path().join("source.part");
  let destination = root.path().join("destination.gguf");
  std::fs::write(&source, b"new").unwrap();
  std::fs::write(&destination, b"old").unwrap();

  tempfile::TempPath::try_from_path(source)
    .unwrap()
    .persist(&destination)
    .unwrap();

  assert_eq!(std::fs::read(destination).unwrap(), b"new");
}

#[tokio::test]
async fn oversized_regular_cache_is_repaired_under_lock() {
  let server = MockServer::start().await;
  let bytes = b"valid";
  mount_model(&server, bytes).await;
  let root = tempfile::tempdir().unwrap();
  let model = preset(bytes, 16);
  std::fs::write(root.path().join(model.filename), vec![b'x'; 32]).unwrap();

  let path = ensure_model_from_endpoint(root.path(), &model, &server.uri())
    .await
    .unwrap_or_else(|error| panic!("{error:#}"));

  assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[tokio::test]
async fn invalid_regular_cache_is_atomically_replaced() {
  let server = MockServer::start().await;
  let bytes = b"new-valid-model";
  mount_model(&server, bytes).await;
  let root = tempfile::tempdir().unwrap();
  let model = preset(bytes, 1024);
  std::fs::write(root.path().join(model.filename), b"old-invalid-model").unwrap();

  let path = ensure_model_from_endpoint(root.path(), &model, &server.uri())
    .await
    .unwrap();

  assert_eq!(std::fs::read(path).unwrap(), bytes);
  assert_no_staging(root.path());
}

#[cfg(windows)]
#[tokio::test]
async fn failed_atomic_persist_preserves_existing_regular_file() {
  use std::os::windows::fs::OpenOptionsExt;

  let server = MockServer::start().await;
  let bytes = b"replacement";
  mount_model(&server, bytes).await;
  let root = tempfile::tempdir().unwrap();
  let model = preset(bytes, 1024);
  let destination = root.path().join(model.filename);
  let original = b"original";
  std::fs::write(&destination, original).unwrap();
  let _deny_delete = std::fs::OpenOptions::new()
    .read(true)
    .share_mode(0x1 | 0x2)
    .open(&destination)
    .unwrap();

  let result = ensure_model_from_endpoint(root.path(), &model, &server.uri()).await;

  assert!(result.is_err());
  assert_eq!(std::fs::read(destination).unwrap(), original);
  assert_no_staging(root.path());
}

#[tokio::test]
async fn cleanup_failure_returns_error_without_publishing_ready() {
  let root = tempfile::tempdir().unwrap();
  let staging = root.path().join("not-a-directory");
  let destination = root.path().join("final.gguf");
  std::fs::write(&staging, b"cleanup blocker").unwrap();
  std::fs::write(&destination, b"complete model").unwrap();
  let (progress, observed) = watch::channel(ModelDownloadProgress::new(
    DownloadPhase::Installing,
    14,
    Some(14),
  ));

  let error = finalize_download_after_cleanup(&staging, Ok(destination), &progress)
    .await
    .unwrap_err();

  assert!(error.to_string().contains("clean"));
  assert_eq!(observed.borrow().phase, DownloadPhase::Installing);
}

#[tokio::test]
async fn stale_target_staging_is_removed_after_lock_acquisition() {
  let server = MockServer::start().await;
  let bytes = b"post-crash-model";
  mount_model(&server, bytes).await;
  let root = tempfile::tempdir().unwrap();
  let model = preset(bytes, 1024);
  let key = model_destination_key(model.filename).unwrap();
  let stale = root.path().join(format!(".download-{key}-stale"));
  std::fs::create_dir(&stale).unwrap();
  std::fs::write(stale.join("partial"), b"partial").unwrap();

  ensure_model_from_endpoint(root.path(), &model, &server.uri())
    .await
    .unwrap();

  assert_no_staging(root.path());
}

#[tokio::test]
async fn locks_path_must_be_a_directory() {
  let root = tempfile::tempdir().unwrap();
  std::fs::write(root.path().join(".locks"), b"not a directory").unwrap();
  let model = preset(b"model", 1024);

  let error = ensure_model_from_endpoint(root.path(), &model, "http://127.0.0.1:9")
    .await
    .unwrap_err();

  assert!(error.to_string().contains("lock directory"));
}

#[cfg(unix)]
#[tokio::test]
async fn locks_symlink_is_rejected_without_touching_target() {
  use std::os::unix::fs::symlink;

  let root = tempfile::tempdir().unwrap();
  let target = tempfile::tempdir().unwrap();
  symlink(target.path(), root.path().join(".locks")).unwrap();
  let model = preset(b"model", 1024);

  let error = ensure_model_from_endpoint(root.path(), &model, "http://127.0.0.1:9")
    .await
    .unwrap_err();

  assert!(error.to_string().contains("lock directory"));
  assert!(target.path().is_dir());
}

#[cfg(unix)]
#[tokio::test]
async fn cache_root_symlink_is_rejected() {
  use std::os::unix::fs::symlink;

  let parent = tempfile::tempdir().unwrap();
  let target = tempfile::tempdir().unwrap();
  let cache = parent.path().join("models");
  symlink(target.path(), &cache).unwrap();
  let model = preset(b"model", 1024);

  let error = ensure_model_from_endpoint(&cache, &model, "http://127.0.0.1:9")
    .await
    .unwrap_err();

  assert!(error.to_string().contains("cache directory"));
}
