use agent_core::{AgentEvent, SessionId, ToolCallId, ToolResult, TurnId};
use agent_storage::SessionStore;
use chrono::Utc;

#[tokio::test]
async fn events_are_durable_ordered_and_replayable_after_reopen() {
  let path = std::env::temp_dir().join(format!(
    "lyonbrown-storage-{}-{}.db",
    std::process::id(),
    SessionId::new()
  ));
  let session_id = SessionId::new();
  let turn_id = TurnId::new();
  let now = Utc::now();
  let store = SessionStore::open(&path).await.unwrap();
  let writer = store.writer(session_id);
  writer
    .record(AgentEvent::SessionStarted {
      session_id,
      ts: now,
    })
    .unwrap();
  assert_eq!(store.events(session_id).await.unwrap().len(), 1);
  writer
    .record(AgentEvent::UserMessageReceived {
      session_id,
      turn_id,
      text: "first question".into(),
      ts: now,
    })
    .unwrap();
  writer
    .record(AgentEvent::ToolCompleted {
      step_id: agent_core::StepId::new(),
      result: ToolResult {
        invocation_id: ToolCallId::new(),
        ok: false,
        output: String::new(),
        truncated: false,
        duration_ms: 1,
        error: Some("failed".into()),
      },
      ts: now,
    })
    .unwrap();
  writer.flush().await.unwrap();
  drop(writer);
  drop(store);

  let reopened = SessionStore::open(&path).await.unwrap();
  let events = reopened.events(session_id).await.unwrap();
  assert_eq!(events.len(), 3);
  assert_eq!(events[0].kind(), "SessionStarted");
  assert_eq!(events[1].kind(), "UserMessageReceived");
  assert_eq!(events[2].kind(), "ToolCompleted");
  let sessions = reopened.sessions().await.unwrap();
  assert_eq!(sessions[0].id, session_id);
  assert_eq!(sessions[0].event_count, 3);
  assert_eq!(reopened.tool_audit_count(session_id).await.unwrap(), 1);
  drop(reopened);
  std::fs::remove_file(&path).unwrap();
}

#[tokio::test]
async fn record_reports_database_failure_before_responding() {
  let path = std::env::temp_dir().join(format!(
    "lyonbrown-storage-failure-{}-{}.db",
    std::process::id(),
    SessionId::new()
  ));
  let store = SessionStore::open(&path).await.unwrap();
  std::fs::remove_file(&path).unwrap();
  std::fs::create_dir(&path).unwrap();
  let writer = store.writer(SessionId::new());

  assert!(
    writer
      .record(AgentEvent::SessionStarted {
        session_id: SessionId::new(),
        ts: Utc::now(),
      })
      .is_err()
  );

  assert!(writer.close().await.is_err());
  drop(store);
  std::fs::remove_dir(&path).unwrap();
}
