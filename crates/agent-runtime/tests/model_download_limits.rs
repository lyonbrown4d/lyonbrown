use agent_runtime::model_manager::{ModelPreset, ensure_model_from_endpoint};
use sha2::{Digest, Sha256};
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

async fn mount_metadata(server: &MockServer, size: Option<usize>) {
  let mut response = ResponseTemplate::new(200).insert_header("etag", "\"test-etag\"");
  response = match size {
    Some(size) => response.insert_header("content-length", size.to_string()),
    None => response.insert_header("transfer-encoding", "chunked"),
  };
  Mock::given(method("HEAD"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(response)
    .mount(server)
    .await;
}

fn assert_no_artifacts(root: &std::path::Path) {
  let unexpected = std::fs::read_dir(root)
    .unwrap()
    .filter_map(Result::ok)
    .filter(|entry| entry.file_name() != ".locks")
    .collect::<Vec<_>>();
  assert!(unexpected.is_empty(), "unexpected cache artifacts");
}

#[tokio::test]
async fn metadata_size_limit_rejects_before_get() {
  let server = MockServer::start().await;
  let root = tempfile::tempdir().unwrap();
  let bytes = b"GGUF-too-large";
  let preset = test_preset(bytes, 4);
  mount_metadata(&server, Some(bytes.len())).await;
  Mock::given(method("GET"))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
    .expect(0)
    .mount(&server)
    .await;

  assert!(
    ensure_model_from_endpoint(root.path(), &preset, &server.uri())
      .await
      .is_err()
  );
  assert_no_artifacts(root.path());
}

#[tokio::test]
async fn chunk_limit_cleans_unknown_length_response() {
  let server = MockServer::start().await;
  let root = tempfile::tempdir().unwrap();
  let bytes = b"GGUF-chunked-too-large";
  let preset = test_preset(bytes, 4);
  mount_metadata(&server, None).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200)
        .insert_header("transfer-encoding", "chunked")
        .set_body_bytes(bytes),
    )
    .mount(&server)
    .await;

  let error = ensure_model_from_endpoint(root.path(), &preset, &server.uri())
    .await
    .unwrap_err();

  assert!(error.to_string().contains("safety limit"));
  assert_no_artifacts(root.path());
}

#[tokio::test]
async fn chunk_limit_ignores_misleading_small_metadata() {
  let server = MockServer::start().await;
  let root = tempfile::tempdir().unwrap();
  let bytes = b"GGUF-misleading-too-large";
  let preset = test_preset(bytes, 4);
  mount_metadata(&server, Some(1)).await;
  Mock::given(method("GET"))
    .and(path("/test-owner/test-repo/resolve/main/test.gguf"))
    .respond_with(
      ResponseTemplate::new(200)
        .insert_header("x-linked-size", "1")
        .set_body_bytes(bytes),
    )
    .mount(&server)
    .await;

  let error = ensure_model_from_endpoint(root.path(), &preset, &server.uri())
    .await
    .unwrap_err();

  assert!(error.to_string().contains("safety limit"));
  assert_no_artifacts(root.path());
}

#[tokio::test]
async fn path_traversal_is_rejected_before_network_or_file_access() {
  let server = MockServer::start().await;
  let root = tempfile::tempdir().unwrap();
  let mut preset = test_preset(b"GGUF-traversal", 1024);
  preset.filename = "../escaped.gguf";

  let error = ensure_model_from_endpoint(root.path(), &preset, &server.uri())
    .await
    .unwrap_err();

  assert!(error.to_string().contains("single normal path component"));
  assert!(!root.path().parent().unwrap().join("escaped.gguf").exists());
  assert_no_artifacts(root.path());
}

#[tokio::test]
async fn invalid_digest_is_rejected_before_creating_a_lock_file() {
  let root = tempfile::tempdir().unwrap();
  let mut preset = test_preset(b"GGUF-invalid-digest", 1024);
  preset.sha256 = "../not-a-sha256".to_owned().into();

  let error = ensure_model_from_endpoint(root.path(), &preset, "http://127.0.0.1:9")
    .await
    .unwrap_err();

  assert!(error.to_string().contains("64 hexadecimal"));
  assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
}

#[tokio::test]
async fn uppercase_digest_is_rejected_before_lock_or_network_access() {
  let root = tempfile::tempdir().unwrap();
  let mut preset = test_preset(b"GGUF-uppercase-digest", 1024);
  preset.sha256 = preset.sha256.to_ascii_uppercase().into();

  let error = ensure_model_from_endpoint(root.path(), &preset, "http://127.0.0.1:9")
    .await
    .unwrap_err();

  assert!(error.to_string().contains("lowercase hexadecimal"));
  assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
}

#[tokio::test]
async fn final_destination_directory_is_rejected_without_removal() {
  let root = tempfile::tempdir().unwrap();
  let preset = test_preset(b"GGUF-directory-target", 1024);
  let destination = root.path().join(preset.filename);
  std::fs::create_dir(&destination).unwrap();

  let error = ensure_model_from_endpoint(root.path(), &preset, "http://127.0.0.1:9")
    .await
    .unwrap_err();

  assert!(error.to_string().contains("not a regular file"));
  assert!(destination.is_dir());
}

#[cfg(unix)]
#[tokio::test]
async fn cached_symlink_is_rejected_without_touching_target() {
  use std::os::unix::fs::symlink;

  let root = tempfile::tempdir().unwrap();
  let target_dir = tempfile::tempdir().unwrap();
  let target = target_dir.path().join("target.gguf");
  let bytes = b"GGUF-symlink-target";
  let preset = test_preset(bytes, 1024);
  std::fs::write(&target, bytes).unwrap();
  symlink(&target, root.path().join(preset.filename)).unwrap();

  let error = ensure_model_from_endpoint(root.path(), &preset, "http://127.0.0.1:9")
    .await
    .unwrap_err();

  assert!(error.to_string().contains("not a regular file"));
  assert_eq!(std::fs::read(target).unwrap(), bytes);
}
