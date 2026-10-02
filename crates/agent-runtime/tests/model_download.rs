use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use agent_runtime::model_manager::{
  ModelPreset, ensure_model, ensure_model_from_endpoint, select_preset,
};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const GIB: u64 = 1024 * 1024 * 1024;

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

async fn mount_metadata(server: &MockServer, size: usize, repo_commit: bool) {
  let mut response = ResponseTemplate::new(200)
    .insert_header("content-length", size.to_string())
    .insert_header("etag", "\"test-etag\"");
  if repo_commit {
    response = response.insert_header("x-repo-commit", "test-commit");
  }
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(response)
    .mount(server)
    .await;
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
  mount_metadata(&server, bytes.len(), true).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
    .expect(1)
    .mount(&server)
    .await;
  let root = tempfile::tempdir().unwrap();
  let preset = test_preset(bytes, 1024);

  let path = ensure_model_from_endpoint(root.path(), &preset, &server.uri())
    .await
    .unwrap();

  assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[tokio::test]
async fn download_does_not_require_repo_commit_header() {
  let server = MockServer::start().await;
  let bytes = b"GGUF-without-repo-commit";
  mount_metadata(&server, bytes.len(), false).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
    .expect(1)
    .mount(&server)
    .await;
  let root = tempfile::tempdir().unwrap();
  let preset = test_preset(bytes, 1024);

  let path = ensure_model_from_endpoint(root.path(), &preset, &server.uri())
    .await
    .unwrap();

  assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[tokio::test]
async fn valid_cache_avoids_all_network_requests() {
  let root = tempfile::tempdir().unwrap();
  let bytes = b"GGUF-cached";
  let preset = test_preset(bytes, 1024);
  std::fs::write(root.path().join(preset.filename), bytes).unwrap();

  let path = ensure_model(root.path(), &preset).await.unwrap();

  assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[tokio::test]
async fn failed_hash_removes_all_staging_artifacts() {
  let server = MockServer::start().await;
  let root = tempfile::tempdir().unwrap();
  let preset = test_preset(b"expected", 1024);
  mount_metadata(&server, 5, true).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(b"wrong"))
    .mount(&server)
    .await;

  let error = ensure_model_from_endpoint(root.path(), &preset, &server.uri())
    .await
    .unwrap_err();

  assert!(error.to_string().contains("SHA-256"));
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

#[tokio::test]
async fn concurrent_ensure_for_same_digest_performs_one_get() {
  let server = MockServer::start().await;
  let bytes = Arc::new(vec![b'g'; 256 * 1024]);
  mount_metadata(&server, bytes.len(), true).await;
  let get_count = Arc::new(AtomicUsize::new(0));
  let observed = Arc::clone(&get_count);
  let body = Arc::clone(&bytes);
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(move |_request: &wiremock::Request| {
      observed.fetch_add(1, Ordering::SeqCst);
      ResponseTemplate::new(200).set_body_bytes(body.as_ref().clone())
    })
    .mount(&server)
    .await;
  let root = tempfile::tempdir().unwrap();
  let preset = test_preset(bytes.as_ref(), 512 * 1024);
  let endpoint = server.uri();

  let (first, second) = tokio::join!(
    ensure_model_from_endpoint(root.path(), &preset, &endpoint),
    ensure_model_from_endpoint(root.path(), &preset, &endpoint),
  );

  assert_eq!(first.unwrap(), second.unwrap());
  assert_eq!(get_count.load(Ordering::SeqCst), 1);
}
