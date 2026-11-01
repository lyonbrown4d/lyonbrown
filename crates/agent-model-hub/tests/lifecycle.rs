use agent_model_hub::{ArtifactFile, HubClient, HubModel, Runtime, plan};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use wiremock::{
  Mock, MockServer, ResponseTemplate,
  matchers::{method, path},
};

const REVISION: &str = "0123456789012345678901234567890123456789";
const WEIGHTS: &[u8] = b"GGUF-synthetic-model-bytes";

async fn fixture(wrong_hash: bool) -> (MockServer, tempfile::TempDir, HubClient) {
  let server = MockServer::start().await;
  let directory = tempfile::tempdir().expect("temporary model root");
  let client = HubClient::new(&server.uri(), None, directory.path()).expect("Hub client");
  let info = json!({"id":"test/model","sha":REVISION,"gated":false,"config":{"model_type":"llama"},"cardData":{"license":"apache-2.0"}});
  Mock::given(method("GET"))
    .and(path("/api/models"))
    .respond_with(ResponseTemplate::new(200).set_body_json(json!([info.clone()])))
    .mount(&server)
    .await;
  Mock::given(method("GET"))
    .and(path(format!("/api/models/test/model/revision/{REVISION}")))
    .respond_with(ResponseTemplate::new(200).set_body_json(info))
    .mount(&server)
    .await;
  let hash = if wrong_hash {
    "a".repeat(64)
  } else {
    hex::encode(Sha256::digest(WEIGHTS))
  };
  let files = json!([{"type":"file","oid":"fileoid","path":"model.gguf","size":WEIGHTS.len(),"lfs":{"oid":hash,"size":WEIGHTS.len(),"pointerSize":100}}]);
  Mock::given(method("GET"))
    .and(path(format!("/api/models/test/model/tree/{REVISION}")))
    .respond_with(ResponseTemplate::new(200).set_body_json(files))
    .mount(&server)
    .await;
  Mock::given(method("HEAD"))
    .and(path(format!("/test/model/resolve/{REVISION}/model.gguf")))
    .respond_with(
      ResponseTemplate::new(200)
        .insert_header("content-length", WEIGHTS.len().to_string())
        .insert_header("x-repo-commit", REVISION)
        .insert_header("etag", format!("\"{hash}\"")),
    )
    .mount(&server)
    .await;
  Mock::given(method("GET"))
    .and(path(format!("/test/model/resolve/{REVISION}/model.gguf")))
    .respond_with(ResponseTemplate::new(200).set_body_bytes(WEIGHTS))
    .mount(&server)
    .await;
  (server, directory, client)
}

#[tokio::test]
async fn discovers_installs_pinned_weights_reopens_and_quarantines_without_claiming_ready() {
  let (server, root, client) = fixture(false).await;
  let cancel = CancellationToken::new();
  let search = client
    .search("model", 10, &cancel)
    .await
    .expect("search models");
  assert_eq!(search[0].repository, "test/model");
  let model = client
    .inspect("test/model", REVISION, &cancel)
    .await
    .expect("inspect model");
  assert_eq!(model.license.as_deref(), Some("apache-2.0"));
  let plan = plan(&model, "model.gguf", Runtime::LlamaCpp).expect("plan model");
  assert_eq!(plan.total_bytes, WEIGHTS.len() as u64);
  assert_eq!(
    plan.files[0].sha256.as_deref(),
    Some(hex::encode(Sha256::digest(WEIGHTS)).as_str())
  );
  let installed = client
    .install(&plan, &cancel, None)
    .await
    .expect("install model");
  assert_eq!(installed.state, "installed_unverified");
  assert_eq!(
    std::fs::read(&installed.entrypoint).expect("installed bytes"),
    WEIGHTS
  );
  let reopened = HubClient::new(&server.uri(), None, root.path()).expect("reopen client");
  assert_eq!(reopened.installed().expect("inventory").len(), 1);
  assert_eq!(
    reopened.jobs().expect("durable job")[0].progress.state,
    "installed_unverified"
  );
  std::fs::write(&installed.entrypoint, vec![b'x'; WEIGHTS.len()]).expect("simulate corruption");
  assert!(reopened.installed().is_err());
  std::fs::write(&installed.entrypoint, WEIGHTS).expect("restore artifact");
  reopened.remove(&plan.id).expect("remove model");
  assert!(reopened.installed().expect("empty inventory").is_empty());
  assert_eq!(
    std::fs::read_dir(root.path().join("quarantine"))
      .expect("quarantine")
      .count(),
    1
  );
}

#[tokio::test]
async fn digest_failure_and_tampered_plan_never_activate_artifact() {
  let (_server, root, client) = fixture(true).await;
  let cancel = CancellationToken::new();
  let model = client
    .inspect("test/model", REVISION, &cancel)
    .await
    .expect("inspection");
  let mut plan = plan(&model, "model.gguf", Runtime::LlamaCpp).expect("plan");
  assert!(client.install(&plan, &cancel, None).await.is_err());
  assert!(client.installed().expect("inventory").is_empty());
  assert_eq!(
    client.jobs().expect("failed state")[0].progress.state,
    "failed"
  );
  plan.files[0].path = "../../escape.gguf".into();
  assert!(client.install(&plan, &cancel, None).await.is_err());
  assert!(!root.path().join("escape.gguf").exists());
}

#[tokio::test]
async fn cancellation_prevents_queries_and_activation() {
  let (server, _root, client) = fixture(false).await;
  let cancel = CancellationToken::new();
  cancel.cancel();
  assert!(client.search("model", 10, &cancel).await.is_err());
  assert!(
    client
      .inspect("test/model", REVISION, &cancel)
      .await
      .is_err()
  );
  assert!(
    server
      .received_requests()
      .await
      .expect("requests")
      .is_empty()
  );
}

#[tokio::test]
async fn pinned_partial_download_resumes_but_ignored_ranges_and_corruption_never_activate() {
  for case in ["valid", "ignored", "corrupt"] {
    let (server, root, client) = fixture(false).await;
    let cancel = CancellationToken::new();
    let model = client
      .inspect("test/model", REVISION, &cancel)
      .await
      .expect("inspect");
    let plan = plan(&model, "model.gguf", Runtime::LlamaCpp).expect("plan");
    let staging = root.path().join("staging").join(&plan.id);
    std::fs::create_dir_all(&staging).expect("staging");
    let partial = staging.join("model.gguf.part");
    let offset = 8;
    let prefix = if case == "corrupt" {
      vec![b'x'; offset]
    } else {
      WEIGHTS[..offset].to_vec()
    };
    std::fs::write(&partial, &prefix).expect("interrupted prefix");
    if case != "ignored" {
      Mock::given(method("GET"))
        .and(path(format!("/test/model/resolve/{REVISION}/model.gguf")))
        .and(wiremock::matchers::header(
          "range",
          format!("bytes={offset}-{}", WEIGHTS.len() - 1),
        ))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(WEIGHTS[offset..].to_vec()))
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    }
    let result = client.install(&plan, &cancel, None).await;
    if case == "valid" {
      let installed = result.expect("resume valid prefix");
      assert_eq!(
        std::fs::read(installed.entrypoint).expect("installed"),
        WEIGHTS
      );
    } else {
      assert!(result.is_err(), "{case} must fail closed");
      assert!(client.installed().expect("inventory").is_empty());
      let remaining = std::fs::read(&partial).expect("partial remains");
      if case == "ignored" {
        assert_eq!(remaining, prefix);
      } else {
        assert!(
          remaining.is_empty(),
          "invalid cached prefix must not be retried"
        );
      }
    }
  }
}

#[tokio::test]
async fn inspection_preserves_linked_hash_before_cdn_redirect() {
  let (server, _root, client) = fixture(false).await;
  let expected = hex::encode(Sha256::digest(WEIGHTS));
  Mock::given(method("HEAD"))
    .and(path(format!("/test/model/resolve/{REVISION}/model.gguf")))
    .respond_with(
      ResponseTemplate::new(302)
        .insert_header("location", "https://unreachable.invalid/model.gguf")
        .insert_header("x-linked-size", WEIGHTS.len().to_string())
        .insert_header("x-linked-etag", format!("\"{expected}\""))
        .insert_header("x-repo-commit", REVISION),
    )
    .with_priority(1)
    .mount(&server)
    .await;
  let model = client
    .inspect("test/model", REVISION, &CancellationToken::new())
    .await
    .expect("Hub redirect metadata");
  assert_eq!(model.files[0].sha256.as_deref(), Some(expected.as_str()));
}

#[test]
fn plans_safetensors_closure_and_rejects_unsupported_variants() {
  let files = [
    "config.json",
    "tokenizer.json",
    "model-00001-of-00002.safetensors",
    "model-00002-of-00002.safetensors",
    "model.safetensors.index.json",
    "evil.py",
  ]
  .into_iter()
  .map(|path| ArtifactFile {
    path: path.into(),
    bytes: 10,
    sha256: None,
  })
  .collect();
  let model = HubModel {
    repository: "test/model".into(),
    revision: REVISION.into(),
    license: None,
    gated: false,
    architecture: Some("llama".into()),
    files,
  };
  let result = plan(
    &model,
    "model-00001-of-00002.safetensors",
    Runtime::MistralRs,
  )
  .expect("complete closure");
  assert_eq!(result.files.len(), 5);
  assert_eq!(result.entrypoint, ".");
  assert!(result.files.iter().all(|file| !file.path.ends_with(".py")));
  assert!(
    plan(
      &model,
      "model-00001-of-00002.safetensors",
      Runtime::LlamaCpp
    )
    .is_err()
  );
  let mut unpinned = model.clone();
  unpinned.revision = "main".into();
  assert!(
    plan(
      &unpinned,
      "model-00001-of-00002.safetensors",
      Runtime::MistralRs
    )
    .is_err()
  );
}
