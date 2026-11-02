use agent_core::events::{AGENT_EVENT_KIND, AgentEvent, TokenUsage};
use agent_storage::Store;
use diesel::{connection::SimpleConnection, prelude::*};
use serde_json::json;

#[test]
fn semantic_outbox_failure_rolls_back_snapshot_audit_and_all_events() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let path = directory.path().join("semantic.sqlite");
  let store = Store::open(&path)?;
  let session = store.create_session("semantic", "/tmp", "local")?;
  let (run, _) = store.accept_run(&session.id, 1, "hello", "request")?;
  store.commit_snapshot(&session.id, 1, b"before", "audit", json!({}))?;
  let events_before = store.events(&run.id, 0, 100)?;
  let outbox_before = store.pending_outbox(100)?;
  let semantic = AgentEvent::UsageUpdated {
    entry_id: "assistant".into(),
    usage: TokenUsage {
      input: 5,
      output: 3,
      total: 8,
      ..Default::default()
    },
  };
  let mut connection = SqliteConnection::establish(path.to_str().expect("path"))?;
  // Fail only after snapshot and the original audit event have been inserted.
  connection.batch_execute("CREATE TRIGGER reject_semantic_outbox BEFORE INSERT ON outbox WHEN EXISTS (SELECT 1 FROM events WHERE run_id = NEW.run_id AND cursor = NEW.cursor AND json_extract(record, '$.kind') = 'agent_event') BEGIN SELECT RAISE(ABORT, 'semantic outbox fault'); END;")?;
  let result = store.commit_run_snapshot_with_events(
    &session.id,
    1,
    Some(&run.id),
    b"after",
    "record_appended",
    json!({"record":"usage"}),
    std::slice::from_ref(&semantic),
  );
  assert!(result.is_err());
  assert_eq!(store.load_snapshot(&session.id)?, Some(b"before".to_vec()));
  assert_eq!(store.events(&run.id, 0, 100)?, events_before);
  assert_eq!(store.pending_outbox(100)?, outbox_before);
  connection.batch_execute("DROP TRIGGER reject_semantic_outbox;")?;
  store.commit_run_snapshot_with_events(
    &session.id,
    1,
    Some(&run.id),
    b"after",
    "record_appended",
    json!({"record":"usage"}),
    std::slice::from_ref(&semantic),
  )?;
  assert_eq!(store.load_snapshot(&session.id)?, Some(b"after".to_vec()));
  let events = store.events(&run.id, 0, 100)?;
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
  assert_eq!(store.pending_outbox(100)?, events);
  Ok(())
}

#[test]
fn explicit_run_snapshot_rejects_other_sessions_and_stale_owners() -> anyhow::Result<()> {
  let directory = tempfile::tempdir()?;
  let store = Store::open(directory.path().join("owners.sqlite"))?;
  let session = store.create_session("first", "/tmp", "local")?;
  let other = store.create_session("second", "/tmp", "local")?;
  let (run, _) = store.accept_run(&other.id, 1, "hello", "request")?;
  let before = store.pending_outbox(100)?;
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
        .is_err()
    );
  }
  assert!(store.load_snapshot(&session.id)?.is_none());
  assert_eq!(store.pending_outbox(100)?, before);
  Ok(())
}
