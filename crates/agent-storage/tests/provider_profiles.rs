use agent_core::provider_profiles::{ProviderProfile, RemoteProviderProtocol};
use agent_storage::{ModelBinding, ProviderProfileError, Store};
use sea_orm::prelude::*;

fn profile(id: &str, revision: u64) -> ProviderProfile {
  ProviderProfile {
    id: id.into(),
    name: "Fixture provider".into(),
    protocol: RemoteProviderProtocol::OpenaiResponses,
    base_url: "http://127.0.0.1:8000/v1".into(),
    model: "fixture".into(),
    revision,
    credential_id: "opaque-reference".into(),
  }
}

fn failure<T: std::fmt::Debug>(result: anyhow::Result<T>, expected: ProviderProfileError) {
  assert_eq!(
    result
      .expect_err("expected failure")
      .downcast_ref::<ProviderProfileError>(),
    Some(&expected)
  );
}

#[tokio::test]
async fn cas_persists_and_emits_only_committed_configuration_changes() {
  let dir = tempfile::tempdir().expect("directory");
  let path = dir.path().join("profiles.db");
  let store = Store::open(&path).await.expect("store");
  let first = profile("fixture", 1);
  assert_eq!(
    store.put_provider_profile(&first, 0).await.expect("create"),
    first
  );
  failure(
    store.put_provider_profile(&first, 0).await,
    ProviderProfileError::Conflict,
  );
  let mut updated = profile("fixture", 2);
  updated.model = "updated".into();
  updated.credential_id = "replacement-opaque-reference".into();
  store
    .put_provider_profile(&updated, 1)
    .await
    .expect("update");
  failure(
    store.put_provider_profile(&updated, 1).await,
    ProviderProfileError::Conflict,
  );
  failure(
    store.remove_provider_profile("fixture", 1).await,
    ProviderProfileError::Conflict,
  );
  failure(
    store.put_provider_profile(&profile("missing", 2), 1).await,
    ProviderProfileError::NotFound,
  );
  drop(store);
  let store = Store::open(&path).await.expect("reopen");
  assert_eq!(
    store.provider_profile("fixture").await.expect("profile"),
    Some(updated)
  );
  let events = store
    .events("provider:fixture", 0, 100)
    .await
    .expect("events");
  assert_eq!(events.len(), 2);
  assert!(
    events
      .iter()
      .all(|event| event.kind == "provider_profile_changed")
  );
  assert_eq!(store.pending_outbox(100).await.expect("outbox").len(), 2);
  store
    .remove_provider_profile("fixture", 2)
    .await
    .expect("remove");
  failure(
    store.remove_provider_profile("fixture", 2).await,
    ProviderProfileError::NotFound,
  );
  assert!(
    store
      .provider_profile("fixture")
      .await
      .expect("removed")
      .is_none()
  );
  let events = store
    .events("provider:fixture", 0, 100)
    .await
    .expect("events");
  assert_eq!(events.len(), 3);
  assert_eq!(events[2].kind, "provider_profile_removed");
  assert_eq!(store.pending_outbox(100).await.expect("outbox").len(), 3);
  for event in events {
    let payload: serde_json::Value = serde_json::from_str(&event.payload_json).expect("payload");
    assert_eq!(payload.as_object().expect("object").len(), 2);
    assert_eq!(payload["id"], "fixture");
    assert!(!event.payload_json.contains("opaque-reference"));
  }
}

#[tokio::test]
async fn capacity_is_bounded_but_updates_and_replacements_are_allowed() {
  let store = Store::open(":memory:").await.expect("store");
  for index in (0..100).rev() {
    store
      .put_provider_profile(&profile(&format!("p{index:03}"), 1), 0)
      .await
      .expect("create");
  }
  failure(
    store.put_provider_profile(&profile("overflow", 1), 0).await,
    ProviderProfileError::Capacity,
  );
  let profiles = store.provider_profiles().await.expect("profiles");
  assert_eq!(profiles.len(), 100);
  assert_eq!(profiles.first().expect("first").id, "p000");
  assert_eq!(profiles.last().expect("last").id, "p099");
  store
    .put_provider_profile(&profile("p000", 2), 1)
    .await
    .expect("update at capacity");
  store
    .remove_provider_profile("p000", 2)
    .await
    .expect("delete");
  store
    .put_provider_profile(&profile("replacement", 1), 0)
    .await
    .expect("replace");
  assert_eq!(
    store.provider_profiles().await.expect("profiles").len(),
    100
  );
  assert!(
    store
      .events("provider:overflow", 0, 100)
      .await
      .expect("events")
      .is_empty()
  );
}

#[tokio::test]
async fn invalid_revision_and_fields_leave_no_records_or_events() {
  let store = Store::open(":memory:").await.expect("store");
  for invalid in [profile("fixture", 0), profile("fixture", 2), profile("", 1)] {
    failure(
      store.put_provider_profile(&invalid, 0).await,
      ProviderProfileError::Invalid,
    );
  }
  failure(
    store
      .put_provider_profile(&profile("fixture", u64::MAX), u64::MAX)
      .await,
    ProviderProfileError::Invalid,
  );
  failure(
    store
      .put_provider_profile(&profile("fixture", i64::MAX as u64 + 1), i64::MAX as u64)
      .await,
    ProviderProfileError::Invalid,
  );
  assert!(
    store
      .provider_profiles()
      .await
      .expect("profiles")
      .is_empty()
  );
  assert!(store.pending_outbox(100).await.expect("outbox").is_empty());
}

#[tokio::test]
async fn event_failure_rolls_back_both_profile_changes_and_deletion() {
  let dir = tempfile::tempdir().expect("directory");
  let path = dir.path().join("profiles.db");
  let store = Store::open(&path).await.expect("store");
  let connection = store.connection();
  // Inject a database failure after the profile and event writes, before the outbox write.
  connection.execute_unprepared("CREATE TRIGGER reject_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT, 'injected outbox failure'); END;").await.expect("trigger");
  assert!(
    store
      .put_provider_profile(&profile("fixture", 1), 0)
      .await
      .is_err()
  );
  assert!(
    store
      .provider_profiles()
      .await
      .expect("profiles")
      .is_empty()
  );
  assert!(
    store
      .events("provider:fixture", 0, 100)
      .await
      .expect("events")
      .is_empty()
  );
  connection
    .execute_unprepared("DROP TRIGGER reject_outbox;")
    .await
    .expect("drop trigger");
  store
    .put_provider_profile(&profile("fixture", 1), 0)
    .await
    .expect("create");
  connection.execute_unprepared("CREATE TRIGGER reject_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT, 'injected outbox failure'); END;").await.expect("trigger");
  assert!(
    store
      .put_provider_profile(&profile("fixture", 2), 1)
      .await
      .is_err()
  );
  assert!(store.remove_provider_profile("fixture", 1).await.is_err());
  assert_eq!(
    store
      .provider_profile("fixture")
      .await
      .expect("profile")
      .expect("exists")
      .revision,
    1
  );
  assert_eq!(
    store
      .events("provider:fixture", 0, 100)
      .await
      .expect("events")
      .len(),
    1
  );
  assert_eq!(store.pending_outbox(100).await.expect("outbox").len(), 1);
}

#[tokio::test]
async fn removing_profile_preserves_pinned_run_binding() {
  let store = Store::open(":memory:").await.expect("store");
  store
    .put_provider_profile(&profile("fixture", 1), 0)
    .await
    .expect("create");
  let session = store
    .create_session("fixture", "/tmp", "node")
    .await
    .expect("session");
  store
    .select_model(
      &session.id,
      1,
      &ModelBinding {
        model_id: "fixture".into(),
        execution_id: "configured".into(),
        provider: "openai-responses".into(),
        model: "fixture".into(),
        endpoint: Some("http://127.0.0.1:8000/v1".into()),
        credential_id: "opaque-reference".into(),
      },
    )
    .await
    .expect("select");
  let (run, _) = store
    .accept_run(&session.id, 1, "hello", "once")
    .await
    .expect("run");
  store
    .remove_provider_profile("fixture", 1)
    .await
    .expect("delete");
  assert_eq!(
    store
      .model_binding(&run.id)
      .await
      .expect("binding")
      .expect("pinned")
      .credential_id,
    "opaque-reference"
  );
  assert_eq!(
    store
      .model_binding(&session.id)
      .await
      .expect("binding")
      .expect("selected")
      .model_id,
    "fixture"
  );
}

#[tokio::test]
async fn deleted_identity_cannot_be_recreated_or_updated_by_stale_clients() {
  let dir = tempfile::tempdir().expect("directory");
  let path = dir.path().join("profiles.db");
  let store = Store::open(&path).await.expect("store");
  store
    .put_provider_profile(&profile("fixture", 1), 0)
    .await
    .expect("create");
  store
    .remove_provider_profile("fixture", 1)
    .await
    .expect("remove");
  drop(store);
  let store = Store::open(&path).await.expect("reopen");
  failure(
    store.put_provider_profile(&profile("fixture", 1), 0).await,
    ProviderProfileError::Conflict,
  );
  failure(
    store.put_provider_profile(&profile("fixture", 2), 1).await,
    ProviderProfileError::NotFound,
  );
  assert!(
    store
      .provider_profile("fixture")
      .await
      .expect("profile")
      .is_none()
  );
  assert_eq!(
    store
      .events("provider:fixture", 0, 100)
      .await
      .expect("events")
      .len(),
    2
  );
  assert_eq!(store.pending_outbox(100).await.expect("outbox").len(), 2);
  store
    .put_provider_profile(&profile("new-identity", 1), 0)
    .await
    .expect("new identity");
  assert_eq!(store.provider_profiles().await.expect("profiles").len(), 1);
  assert_eq!(store.pending_outbox(100).await.expect("outbox").len(), 3);
}
