use std::sync::atomic::{AtomicUsize, Ordering};

use agent_cli::model_manager::{
  ModelPreset, ensure_model, ensure_model_from_endpoint, select_preset,
};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);
const GIB: u64 = 1024 * 1024 * 1024;

fn temp_dir() -> std::path::PathBuf {
  let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
  let path = std::env::temp_dir().join(format!(
    "lyonbrown-model-manager-{}-{id}",
    std::process::id()
  ));
  std::fs::create_dir_all(&path).unwrap();
  path
}

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

async fn mount_metadata(server: &MockServer, size: usize) {
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200)
        .insert_header("content-length", size.to_string())
        .insert_header("etag", "\"test-etag\"")
        .insert_header("x-repo-commit", "test-commit"),
    )
    .mount(server)
    .await;
}

async fn mount_metadata_without_repo_commit(server: &MockServer, size: usize) {
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200)
        .insert_header("content-length", size.to_string())
        .insert_header("etag", "\"test-etag\""),
    )
    .mount(server)
    .await;
}

async fn mount_unknown_metadata(server: &MockServer) {
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200)
        .insert_header("transfer-encoding", "chunked")
        .insert_header("etag", "\"test-etag\"")
        .insert_header("x-repo-commit", "test-commit"),
    )
    .mount(server)
    .await;
}

fn assert_cache_empty(root: &std::path::Path, filename: &str) {
  assert!(!root.join(filename).exists());
  assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn automatic_selection_uses_current_available_memory() {
  assert!(select_preset(GIB).is_err());
  assert_eq!(select_preset(2 * GIB).unwrap().id, "qwen3-0.6b-q8_0");
  assert_eq!(select_preset(4 * GIB).unwrap().id, "qwen3-1.7b-q4_k_m");
  assert_eq!(select_preset(8 * GIB).unwrap().id, "qwen3-4b-q4_k_m");
  assert_eq!(select_preset(14 * GIB).unwrap().id, "qwen3-8b-q4_k_m");
}

#[tokio::test]
async fn missing_model_is_downloaded_and_verified() {
  let server = MockServer::start().await;
  let bytes = b"GGUF-test-model";
  mount_metadata(&server, bytes.len()).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
    .expect(1)
    .mount(&server)
    .await;
  let root = temp_dir();
  let preset = test_preset(bytes, 1024);

  let path = ensure_model_from_endpoint(&root, &preset, &server.uri())
    .await
    .unwrap();

  assert_eq!(std::fs::read(path).unwrap(), bytes);
  std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn download_does_not_require_repo_commit_header() {
  let server = MockServer::start().await;
  let bytes = b"GGUF-without-repo-commit";
  mount_metadata_without_repo_commit(&server, bytes.len()).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
    .expect(1)
    .mount(&server)
    .await;
  let root = temp_dir();
  let preset = test_preset(bytes, 1024);

  let path = ensure_model_from_endpoint(&root, &preset, &server.uri())
    .await
    .unwrap();

  assert_eq!(std::fs::read(path).unwrap(), bytes);
  std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn valid_cache_avoids_network_and_invalid_download_is_cleaned_up() {
  let server = MockServer::start().await;
  let root = temp_dir();
  let bytes = b"GGUF-cached";
  let preset = test_preset(bytes, 1024);
  std::fs::write(root.join(preset.filename), bytes).unwrap();

  ensure_model(&root, &preset).await.unwrap();

  let bad = test_preset(bytes, 1024);
  mount_metadata(&server, b"wrong".len()).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(b"wrong"))
    .expect(1)
    .mount(&server)
    .await;
  std::fs::remove_file(root.join(bad.filename)).unwrap();
  assert!(
    ensure_model_from_endpoint(&root, &bad, &server.uri())
      .await
      .is_err()
  );
  assert!(!root.join(bad.filename).exists());
  assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
  std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn oversized_response_is_rejected_before_writing_final_file() {
  let server = MockServer::start().await;
  let root = temp_dir();
  let bytes = b"GGUF-too-large";
  let preset = test_preset(bytes, 4);
  mount_metadata(&server, bytes.len()).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
    .expect(0)
    .mount(&server)
    .await;

  assert!(
    ensure_model_from_endpoint(&root, &preset, &server.uri())
      .await
      .is_err()
  );
  assert_cache_empty(&root, preset.filename);
  std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn oversized_chunked_response_without_content_length_is_stopped_and_cleaned_up() {
  let server = MockServer::start().await;
  let root = temp_dir();
  let bytes = b"GGUF-chunked-too-large";
  let preset = test_preset(bytes, 4);
  mount_unknown_metadata(&server).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200)
        .insert_header("transfer-encoding", "chunked")
        .set_body_bytes(bytes),
    )
    .expect(1)
    .mount(&server)
    .await;

  let error = ensure_model_from_endpoint(&root, &preset, &server.uri())
    .await
    .unwrap_err();

  assert!(error.to_string().contains("safety limit"));
  assert_cache_empty(&root, preset.filename);
  std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn oversized_response_with_misleading_size_header_is_stopped_and_cleaned_up() {
  let server = MockServer::start().await;
  let root = temp_dir();
  let bytes = b"GGUF-misleading-too-large";
  let preset = test_preset(bytes, 4);
  mount_metadata(&server, 1).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200)
        .insert_header("x-linked-size", "1")
        .set_body_bytes(bytes),
    )
    .expect(1)
    .mount(&server)
    .await;

  let error = ensure_model_from_endpoint(&root, &preset, &server.uri())
    .await
    .unwrap_err();

  assert!(error.to_string().contains("safety limit"));
  assert_cache_empty(&root, preset.filename);
  std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn filename_with_path_components_is_rejected_before_network_or_file_access() {
  let server = MockServer::start().await;
  let root = temp_dir();
  let bytes = b"GGUF-traversal";
  let mut preset = test_preset(bytes, 1024);
  preset.filename = "../escaped.gguf";

  let error = ensure_model_from_endpoint(&root, &preset, &server.uri())
    .await
    .unwrap_err();

  assert!(error.to_string().contains("single normal path component"));
  assert!(!root.parent().unwrap().join("escaped.gguf").exists());
  assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
  std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn cached_symlink_is_rejected_without_touching_its_target() {
  use std::os::unix::fs::symlink;

  let server = MockServer::start().await;
  let root = temp_dir();
  let target = root.with_extension("target");
  let bytes = b"GGUF-symlink-target";
  let preset = test_preset(bytes, 1024);
  std::fs::write(&target, bytes).unwrap();
  symlink(&target, root.join(preset.filename)).unwrap();

  let error = ensure_model_from_endpoint(&root, &preset, &server.uri())
    .await
    .unwrap_err();

  assert!(error.to_string().contains("not a regular file"));
  assert_eq!(std::fs::read(&target).unwrap(), bytes);
  assert!(root.join(preset.filename).symlink_metadata().is_ok());
  std::fs::remove_file(root.join(preset.filename)).unwrap();
  std::fs::remove_file(target).unwrap();
  std::fs::remove_dir_all(root).unwrap();
}
