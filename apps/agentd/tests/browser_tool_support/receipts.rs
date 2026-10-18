use crate::support::TestNode;
use agent_core::Event;
use agent_protocol::v1;
use serde_json::{Value, json};

/// Match the model action, excluding authenticated human setup operations.
pub fn assert_receipts(
  node: &TestNode,
  pending: &v1::Approval,
  approval_status: &str,
  outcome: Option<&str>,
) {
  let store = &node.daemon.store;
  let exact: Value = serde_json::from_str(&pending.arguments_json).unwrap();
  let action = exact["action_id"].as_str().unwrap();
  assert_eq!(
    store.approval(&pending.id).unwrap().unwrap().status,
    approval_status
  );
  assert_eq!(
    store.browser_action_status(action).unwrap().as_deref(),
    outcome
  );
  let run_events = store.events(&pending.run_id, 0, 1000).unwrap();
  let authorized: Vec<_> = run_events
    .iter()
    .filter(|event| event.kind == "tool_authorized" && payload(event)["approval_id"] == pending.id)
    .collect();
  let browser_stream = format!("browser:{}", node.session_id);
  let browser_events = store.events(&browser_stream, 0, 1000).unwrap();
  let action_events: Vec<_> = browser_events
    .iter()
    .filter(|event| payload(event)["action_id"] == action)
    .collect();
  let Some(status) = outcome else {
    assert!(
      authorized.is_empty(),
      "rejected action must not consume approval"
    );
    assert!(
      action_events.is_empty(),
      "rejected action must not record intent"
    );
    return;
  };
  assert_eq!(authorized.len(), 1, "one authorization per exact approval");
  let authorized = authorized[0];
  assert_eq!(authorized.run_id, pending.run_id);
  assert_eq!(
    payload(authorized),
    json!({"approval_id":pending.id,"tool":"browser_act"})
  );
  assert_eq!(
    action_events.len(),
    2,
    "one intent and one terminal receipt"
  );
  let intent = action_events[0];
  assert_eq!(intent.run_id, browser_stream);
  assert_eq!(intent.kind, "browser_action_intent");
  assert_eq!(
    payload(intent),
    json!({"action_id":action,"kind":"browser_command"})
  );
  let finished = action_events[1];
  assert_eq!(finished.run_id, browser_stream);
  assert_eq!(finished.kind, "browser_action_finished");
  assert_eq!(
    payload(finished),
    json!({"action_id":action,"status":status})
  );
  let outbox = store.pending_outbox(10_000).unwrap();
  for event in [authorized, intent, finished] {
    let copies: Vec<_> = outbox
      .iter()
      .filter(|item| item.run_id == event.run_id && item.cursor == event.cursor)
      .collect();
    assert_eq!(
      copies,
      vec![event],
      "durable receipt must match its Outbox event"
    );
  }
}

fn payload(event: &Event) -> Value {
  serde_json::from_str(&event.payload_json).unwrap()
}
