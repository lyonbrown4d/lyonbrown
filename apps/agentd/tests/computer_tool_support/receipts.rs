use crate::support::TestNode;
use agent_protocol::v1;
use serde_json::{Value, json};

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
  let operation = store.operation(action).unwrap();
  assert_eq!(
    operation
      .as_ref()
      .map(|operation| operation.status.as_str()),
    outcome
  );
  let events = store.events(&pending.run_id, 0, 1000).unwrap();
  let related: Vec<_> = events
    .iter()
    .filter(|event| {
      let payload: Value = serde_json::from_str(&event.payload_json).unwrap();
      (event.kind == "tool_authorized" && payload["approval_id"] == pending.id)
        || payload["action_id"] == action
    })
    .collect();
  let Some(status) = outcome else {
    assert!(
      related.is_empty(),
      "denied action has no authorization, intent or effect receipt"
    );
    return;
  };
  let operation = operation.unwrap();
  assert_eq!(operation.kind, "computer_act");
  assert_eq!(operation.run_id, pending.run_id);
  assert_eq!(operation.owner_epoch, 1);
  assert!(operation.finished_at_ms.is_some());
  assert_eq!(
    related.len(),
    3,
    "exactly one authorization, intent and terminal receipt"
  );
  let expected = [
    (
      "tool_authorized",
      json!({"approval_id":pending.id,"tool":"computer_act"}),
    ),
    (
      "operation_intent",
      json!({"action_id":action,"kind":"computer_act"}),
    ),
    (
      "operation_finished",
      json!({"action_id":action,"status":status,"summary":{"status":status}}),
    ),
  ];
  let outbox = store.pending_outbox(10_000).unwrap();
  for (event, (kind, payload)) in related.iter().zip(expected) {
    assert_eq!(event.kind, kind);
    assert_eq!(
      serde_json::from_str::<Value>(&event.payload_json).unwrap(),
      payload
    );
    let copies: Vec<_> = outbox
      .iter()
      .filter(|copy| copy.run_id == event.run_id && copy.cursor == event.cursor)
      .collect();
    assert_eq!(
      copies,
      vec![*event],
      "event and durable Outbox copy match exactly"
    );
  }
  assert!(
    !related
      .iter()
      .any(|event| event.payload_json.contains("Synthetic submit")
        || event.payload_json.contains("Untrusted fixture window"))
  );
}
