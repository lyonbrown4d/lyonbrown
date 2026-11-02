use agent_core::events::{AGENT_EVENT_KIND, AgentEvent, TokenUsage};
use agent_storage::Store;
use sea_orm::prelude::*;
use serde_json::json;

#[tokio::test]
async fn semantic_outbox_failure_rolls_back_snapshot_audit_and_all_events() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("semantic.sqlite");
  let store = Store::open(&path).await?;
  let session = store.create_session("semantic", "/tmp", "local").await?;
  let (run, _) = store.accept_run(&session.id, 1, "hello", "request").await?;
  store
    .commit_snapshot(&session.id, 1, b"before", "audit", json!({}))
    .await?;
  let events_before = store.events(&run.id, 0, 100).await?;
  let outbox_before = store.pending_outbox(100).await?;
  let semantic = AgentEvent::UsageUpdated {
    entry_id: "assistant".into(),
    usage: TokenUsage {
      input: 5,
      output: 3,
      total: 8,
      ..Default::default()
    },
  };
  let connection = store.connection();
  // Fail only after snapshot and the original audit event have been inserted.
  connection.execute_unprepared("CREATE TRIGGER reject_semantic_outbox BEFORE INSERT ON outbox WHEN EXISTS (SELECT 1 FROM events WHERE run_id = NEW.run_id AND cursor = NEW.cursor AND json_extract(record, '$.kind') = 'agent_event') BEGIN SELECT RAISE(ABORT, 'semantic outbox fault'); END;").await?;
  let result = store
    .commit_run_snapshot_with_events(
      &session.id,
      1,
      Some(&run.id),
      b"after",
      "record_appended",
      json!({"record":"usage"}),
      std::slice::from_ref(&semantic),
    )
    .await;
  assert!(result.is_err());
  assert_eq!(
    store.load_snapshot(&session.id).await?,
    Some(b"before".to_vec())
  );
  assert_eq!(store.events(&run.id, 0, 100).await?, events_before);
  assert_eq!(store.pending_outbox(100).await?, outbox_before);
  connection
    .execute_unprepared("DROP TRIGGER reject_semantic_outbox;")
    .await?;
  store
    .commit_run_snapshot_with_events(
      &session.id,
      1,
      Some(&run.id),
      b"after",
      "record_appended",
      json!({"record":"usage"}),
      std::slice::from_ref(&semantic),
    )
    .await?;
  assert_eq!(
    store.load_snapshot(&session.id).await?,
    Some(b"after".to_vec())
  );
  let events = store.events(&run.id, 0, 100).await?;
  let added = &events[events_before.len()..];
  assert_eq!(
    added
      .iter()
      .map(|event| event.kind.as_str())
      .collect::<Vec<_>>(),
    ["record_appended", AGENT_EVENT_KIND]
  );
  assert_eq!(
    serde_json::from_str::<AgentEvent>(&added[1].payload_json)?,
    semantic
  );
  assert_eq!(store.pending_outbox(100).await?, events);
  Ok(())
}

#[tokio::test]
async fn explicit_run_snapshot_rejects_other_sessions_and_stale_owners() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let store = Store::open(directory.path().join("owners.sqlite")).await?;
  let session = store.create_session("first", "/tmp", "local").await?;
  let other = store.create_session("second", "/tmp", "local").await?;
  let (run, _) = store.accept_run(&other.id, 1, "hello", "request").await?;
  let before = store.pending_outbox(100).await?;
  for epoch in [1, 2] {
    assert!(
      store
        .commit_run_snapshot_with_events(
          &session.id,
          epoch,
          Some(&run.id),
          b"invalid",
          "audit",
          json!({}),
          &[],
        )
        .await
        .is_err()
    );
  }
  assert!(store.load_snapshot(&session.id).await?.is_none());
  assert_eq!(store.pending_outbox(100).await?, before);
  Ok(())
}
