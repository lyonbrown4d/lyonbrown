use gateway_core::{AccessControl, AccessPrincipal, AuditRecord, AuditStatus, TokenUsage};
use gateway_storage::{GatewayStorage, StorageError};
use std::{
  collections::BTreeSet,
  time::{SystemTime, UNIX_EPOCH},
};

fn now() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .expect("clock")
    .as_secs()
}
fn intent(id: &str, principal: &AccessPrincipal, reserved_tokens: u64) -> AuditRecord {
  AuditRecord {
    request_id: id.into(),
    model: "local".into(),
    principal: Some(principal.clone()),
    reserved_tokens,
    usage: None,
    status: AuditStatus::Pending,
    elapsed_ms: 0,
    policy_revision: "v1".into(),
  }
}
async fn principal(store: &GatewayStorage, budget: u64) -> AccessPrincipal {
  let key = store
    .create_scoped_access_token(
      "scoped",
      now(),
      3600,
      "project-a",
      BTreeSet::from(["local".into()]),
      Some(100),
      Some(budget),
    )
    .await
    .expect("key");
  AccessControl::authenticate(store, key.token())
    .await
    .expect("authenticate")
    .expect("principal")
}
async fn charged(store: &GatewayStorage) -> u64 {
  store.list_access_tokens().await.expect("keys")[0].tokens_charged
}

#[tokio::test]
async fn concurrent_admission_reserves_last_budget_atomically_and_refusal_has_no_audit() {
  let dir = tempfile::tempdir().expect("directory");
  let store = GatewayStorage::open(dir.path().join("db")).expect("open");
  let principal = principal(&store, 100).await;
  let (one, two) = tokio::join!(
    store.record_audit(intent("one", &principal, 100)),
    store.record_audit(intent("two", &principal, 100))
  );
  assert_ne!(one.is_ok(), two.is_ok());
  assert!(matches!(
    if one.is_err() { one } else { two },
    Err(StorageError::BudgetExceeded)
  ));
  assert_eq!(charged(&store).await, 100);
  assert_eq!(store.list_audits(10, 0).await.expect("audits").total, 1);
}

#[tokio::test]
async fn terminal_usage_replaces_reservation_once_and_does_not_add_cached_or_reasoning_subsets() {
  let dir = tempfile::tempdir().expect("directory");
  let store = GatewayStorage::open(dir.path().join("db")).expect("open");
  let principal = principal(&store, 100).await;
  let mut record = intent("one", &principal, 100);
  store.record_audit(record.clone()).await.expect("reserve");
  record.status = AuditStatus::Succeeded;
  record.usage = Some(TokenUsage {
    input_tokens: 20,
    output_tokens: 30,
    cached_input_tokens: 10,
    reasoning_tokens: 15,
  });
  store.record_audit(record.clone()).await.expect("settle");
  store
    .record_audit(record.clone())
    .await
    .expect("idempotent terminal");
  assert_eq!(charged(&store).await, 50);
  assert_eq!(
    store.list_audits(10, 0).await.expect("audits").records[0].usage,
    record.usage
  );
  record.usage.as_mut().expect("usage").output_tokens = 31;
  assert!(matches!(
    store.record_audit(record).await,
    Err(StorageError::Conflict)
  ));
  store
    .record_audit(intent("two", &principal, 50))
    .await
    .expect("remaining budget");
  assert_eq!(charged(&store).await, 100);
}

#[tokio::test]
async fn missing_usage_and_restart_uncertainty_keep_conservative_reservations() {
  let dir = tempfile::tempdir().expect("directory");
  let path = dir.path().join("db");
  let store = GatewayStorage::open(&path).expect("open");
  let principal = principal(&store, 100).await;
  let mut no_usage = intent("no-usage", &principal, 40);
  store.record_audit(no_usage.clone()).await.expect("intent");
  no_usage.status = AuditStatus::Failed;
  store.record_audit(no_usage).await.expect("failed");
  store
    .record_audit(intent("unfinished", &principal, 60))
    .await
    .expect("pending");
  drop(store);
  let reopened = GatewayStorage::open(&path).expect("reopen");
  assert_eq!(charged(&reopened).await, 100);
  let page = reopened.list_audits(10, 0).await.expect("audits");
  assert!(matches!(page.records[0].status, AuditStatus::Uncertain));
  assert_eq!(page.records[0].principal, Some(principal.clone()));
  assert_eq!(page.records[0].reserved_tokens, 60);
  assert!(matches!(
    reopened
      .record_audit(intent("refused", &principal, 1))
      .await,
    Err(StorageError::BudgetExceeded)
  ));
}

#[tokio::test]
async fn revoked_or_forged_scope_is_rejected_at_atomic_admission() {
  let dir = tempfile::tempdir().expect("directory");
  let store = GatewayStorage::open(dir.path().join("db")).expect("open");
  let principal = principal(&store, 100).await;
  let mut forged = principal.clone();
  forged.project_id = "other-project".into();
  assert!(matches!(
    store.record_audit(intent("forged", &forged, 10)).await,
    Err(StorageError::Unauthorized)
  ));
  let mut denied_model = intent("model", &principal, 10);
  denied_model.model = "other-model".into();
  assert!(matches!(
    store.record_audit(denied_model).await,
    Err(StorageError::Unauthorized)
  ));
  store
    .revoke_access_token(&principal.key_id)
    .await
    .expect("revoke");
  assert!(matches!(
    store.record_audit(intent("revoked", &principal, 10)).await,
    Err(StorageError::Unauthorized)
  ));
  assert_eq!(charged(&store).await, 0);
  assert_eq!(store.list_audits(10, 0).await.expect("audits").total, 0);
}

#[tokio::test]
async fn overflowing_reservations_and_usage_fail_without_mutating_balance_or_audit() {
  let dir = tempfile::tempdir().expect("directory");
  let store = GatewayStorage::open(dir.path().join("db")).expect("open");
  let principal = principal(&store, i64::MAX as u64).await;
  assert!(
    store
      .record_audit(intent("overflow-reserve", &principal, u64::MAX))
      .await
      .is_err()
  );
  let mut record = intent("one", &principal, 100);
  store.record_audit(record.clone()).await.expect("intent");
  record.status = AuditStatus::Succeeded;
  record.usage = Some(TokenUsage {
    input_tokens: u64::MAX,
    output_tokens: 1,
    ..TokenUsage::default()
  });
  assert!(store.record_audit(record).await.is_err());
  assert_eq!(charged(&store).await, 100);
  assert!(matches!(
    store.list_audits(10, 0).await.expect("audits").records[0].status,
    AuditStatus::Pending
  ));
}

#[tokio::test]
async fn legacy_key_defaults_and_expiration_remain_compatible() {
  let dir = tempfile::tempdir().expect("directory");
  let store = GatewayStorage::open(dir.path().join("db")).expect("open");
  let key = store
    .create_access_token("legacy", 1000, 60)
    .await
    .expect("key");
  assert_eq!(key.metadata.project_id, "default");
  assert!(key.metadata.allowed_models.is_empty());
  assert_eq!(key.metadata.token_budget, None);
  assert_eq!(key.metadata.max_output_tokens, None);
  let principal = store
    .authenticate_access_token(key.token(), 1000)
    .await
    .expect("authenticate")
    .expect("principal");
  assert_eq!(principal.key_id, key.metadata.id);
  assert!(
    store
      .authenticate_access_token(key.token(), 1060)
      .await
      .expect("expiration")
      .is_none()
  );
  assert!(matches!(
    store.record_audit(intent("expired", &principal, 1)).await,
    Err(StorageError::Unauthorized)
  ));
  assert_eq!(charged(&store).await, 0);
}

#[tokio::test]
async fn duplicate_intent_rolls_back_reservation_and_explicit_uncertainty_keeps_it() {
  let dir = tempfile::tempdir().expect("directory");
  let store = GatewayStorage::open(dir.path().join("db")).expect("open");
  let principal = principal(&store, 100).await;
  let mut record = intent("one", &principal, 40);
  store.record_audit(record.clone()).await.expect("intent");
  assert!(matches!(
    store.record_audit(record.clone()).await,
    Err(StorageError::Conflict)
  ));
  assert_eq!(charged(&store).await, 40);
  record.status = AuditStatus::Uncertain;
  record.usage = Some(TokenUsage {
    input_tokens: 2,
    output_tokens: 3,
    ..TokenUsage::default()
  });
  store.record_audit(record.clone()).await.expect("uncertain");
  store
    .record_audit(record)
    .await
    .expect("idempotent uncertainty");
  assert_eq!(charged(&store).await, 40);
}
