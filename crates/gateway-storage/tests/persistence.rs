use decision_core::{ModelTier, Policy};
use gateway_core::{AuditRecord, AuditSink, AuditStatus, ModelConfig, Secret};
use gateway_storage::{GatewayStorage, StorageError};

fn audit(id: &str, status: AuditStatus) -> AuditRecord {
  AuditRecord {
    request_id: id.into(),
    principal: None,
    reserved_tokens: 0,
    usage: None,
    model: "local".into(),
    status,
    elapsed_ms: 20,
    policy_revision: "rules-v1".into(),
  }
}
fn model() -> ModelConfig {
  ModelConfig {
    id: "local".into(),
    upstream_model: "demo-model".into(),
    api_base: "http://127.0.0.1:8080/v1".into(),
    api_key: Some(Secret::new("fixture-provider-secret-never-persist")),
    api_key_env: Some("TEST_PROVIDER_KEY".into()),
    tier: ModelTier::Balanced,
    local_inference: true,
    capabilities: ["tools".into()].into(),
    context_tokens: Some(8192),
  }
}

#[tokio::test]
async fn audit_intent_terminal_and_restart_uncertainty_are_durable() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let store = GatewayStorage::open(&path).expect("open");
  assert!(matches!(
    GatewayStorage::open(&path),
    Err(StorageError::Conflict)
  ));
  assert!(
    store
      .record_audit(audit("missing", AuditStatus::Succeeded))
      .await
      .is_err()
  );
  AuditSink::record(&store, audit("completed", AuditStatus::Pending))
    .await
    .expect("intent");
  assert!(
    store
      .record_audit(audit("completed", AuditStatus::Pending))
      .await
      .is_err()
  );
  store
    .record_audit(audit("completed", AuditStatus::Succeeded))
    .await
    .expect("terminal");
  store
    .record_audit(audit("completed", AuditStatus::Succeeded))
    .await
    .expect("idempotent terminal");
  assert!(
    store
      .record_audit(audit("completed", AuditStatus::Failed))
      .await
      .is_err()
  );
  store
    .record_audit(audit("unfinished", AuditStatus::Pending))
    .await
    .expect("intent");
  let page = store.list_audits(1, 0).await.expect("page");
  assert_eq!(page.total, 2);
  assert_eq!(page.records[0].request_id, "unfinished");
  drop(store);
  let reopened = GatewayStorage::open(&path).expect("reopen");
  let records = reopened.list_audits(20, 0).await.expect("records").records;
  assert!(matches!(records[0].status, AuditStatus::Uncertain));
  assert!(matches!(records[1].status, AuditStatus::Succeeded));
  assert!(
    reopened
      .record_audit(audit("unfinished", AuditStatus::Pending))
      .await
      .is_err()
  );
  assert_eq!(
    reopened
      .list_audits(1, 1)
      .await
      .expect("second page")
      .records[0]
      .request_id,
    "completed"
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
      std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777,
      0o600
    );
  }
}

#[tokio::test]
async fn versioned_policy_compare_and_swap_retains_the_winning_complete_snapshot() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let store = GatewayStorage::open(&path).expect("open");
  store
    .save_policy(Policy::default(), None)
    .await
    .expect("initial");
  let one = Policy {
    revision: "one".into(),
    default_tier: ModelTier::Fast,
    ..Policy::default()
  };
  let two = Policy {
    revision: "two".into(),
    default_tier: ModelTier::Advanced,
    ..Policy::default()
  };
  let (first, second) = tokio::join!(
    store.save_policy(one.clone(), Some("rules-v1".into())),
    store.save_policy(two.clone(), Some("rules-v1".into()))
  );
  assert_ne!(first.is_ok(), second.is_ok());
  let current = store.load_policy().await.expect("load").expect("policy");
  assert_eq!(current, if first.is_ok() { one } else { two });
  let modified_same_revision = Policy {
    default_tier: ModelTier::Fast,
    ..Policy::default()
  };
  assert!(
    store
      .save_policy(modified_same_revision, Some(current.revision.clone()))
      .await
      .is_err()
  );
  let invalid = Policy {
    revision: "invalid\nrevision".into(),
    ..Policy::default()
  };
  assert!(matches!(
    store
      .save_policy(invalid, Some(current.revision.clone()))
      .await,
    Err(StorageError::InvalidInput("policy revision"))
  ));
  assert_eq!(
    store.load_policy().await.expect("unchanged head"),
    Some(current.clone())
  );
  drop(store);
  let reopened = GatewayStorage::open(path).expect("reopen");
  assert_eq!(reopened.load_policy().await.expect("load"), Some(current));
}

#[tokio::test]
async fn model_snapshots_never_persist_credentials_and_active_revision_can_be_reselected() {
  let directory = tempfile::tempdir().expect("directory");
  let path = directory.path().join("gateway.sqlite3");
  let store = GatewayStorage::open(&path).expect("open");
  let original = model();
  store
    .seed_models("catalog-v1", std::slice::from_ref(&original))
    .await
    .expect("seed");
  let mut replacement = original.clone();
  replacement.upstream_model = "replacement".into();
  assert!(
    store
      .seed_models("catalog-v1", std::slice::from_ref(&replacement))
      .await
      .is_err()
  );
  store
    .seed_models("catalog-v2", &[replacement])
    .await
    .expect("new revision");
  store
    .seed_models("catalog-v1", &[original])
    .await
    .expect("reselect immutable version");
  let loaded = store.load_models().await.expect("load").expect("snapshot");
  assert_eq!(loaded.revision, "catalog-v1");
  assert_eq!(loaded.models[0].upstream_model, "demo-model");
  assert!(
    !serde_json::to_string(&loaded)
      .expect("json")
      .contains("secret")
  );
  drop(store);
  for entry in std::fs::read_dir(directory.path()).expect("read directory") {
    let entry = entry.expect("entry");
    if entry.file_type().expect("type").is_file() {
      let bytes = std::fs::read(entry.path()).expect("read database file");
      assert!(
        !bytes
          .windows(b"fixture-provider-secret-never-persist".len())
          .any(|w| w == b"fixture-provider-secret-never-persist")
      );
    }
  }
}
