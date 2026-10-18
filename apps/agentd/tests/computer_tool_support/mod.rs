mod bridge;
mod failures;
mod receipts;
mod runtime;
mod scheduling;
pub use bridge::LostReceiptHost;
pub use receipts::assert_receipts;
pub use runtime::Runtime;

use crate::support::TestNode;
use agent_protocol::v1;
use agent_test_support::MockModelServer;
use serde_json::{Value, json};
use std::time::Duration;

pub fn route() -> String {
  json!({"task_kind":"desktop","model_policy":"general",
    "allowed_tools":["computer_observe","computer_act"],"execution_mode":"local",
    "target_hint":null,"risk_level":"high","confidence":0.99})
  .to_string()
}
pub async fn wait_requests(model: &MockModelServer, count: usize) {
  tokio::time::timeout(Duration::from_secs(5), model.wait_for_requests(count))
    .await
    .unwrap();
}
pub async fn observation(model: &MockModelServer) -> Value {
  wait_requests(model, 2).await;
  find_observation(&model.requests().await[1].body).expect("structured computer observation")
}
fn find_observation(value: &Value) -> Option<Value> {
  match value {
    Value::String(text) => serde_json::from_str::<Value>(text).ok().filter(|value| {
      value["untrusted_desktop_content"] == true && value["observation"].is_object()
    }),
    Value::Array(values) => values.iter().find_map(find_observation),
    Value::Object(values) => values.values().find_map(find_observation),
    _ => None,
  }
}
pub fn click(observed: &Value) -> Value {
  json!({"snapshot_id":observed["observation"]["snapshot_id"],
    "revision":observed["observation"]["revision"],
    "action":{"kind":"click","x":10,"y":10,"button":"left"},"timeout_ms":5000})
}
pub async fn approval(node: &TestNode, run: &str) -> v1::Approval {
  tokio::time::timeout(Duration::from_secs(5), async {
    let mut events = node.client.watch_run(run, 0).await.unwrap();
    loop {
      if let Some(approval) = node.client.list_approvals(run).await.unwrap().first() {
        return approval.clone();
      }
      assert!(
        events.message().await.unwrap().is_some(),
        "run ended before computer approval"
      );
    }
  })
  .await
  .unwrap()
}
pub fn exact_approval(node: &TestNode, pending: &v1::Approval, host: &str, args: &Value) -> Value {
  let exact: Value = serde_json::from_str(&pending.arguments_json).unwrap();
  assert_eq!(exact["session_id"], node.session_id);
  assert_eq!(exact["owner_epoch"], 1);
  assert_eq!(exact["timeout_ms"], 5000);
  assert_eq!(exact["run_id"], pending.run_id);
  let started: Vec<Value> = node
    .daemon
    .store
    .events(&pending.run_id, 0, 1000)
    .unwrap()
    .into_iter()
    .filter(|event| event.kind == "tool_started")
    .map(|event| serde_json::from_str(&event.payload_json).unwrap())
    .filter(|event: &Value| event["name"] == "computer_act")
    .collect();
  assert_eq!(started.len(), 1);
  assert_eq!(started[0]["arguments"], *args);
  assert_eq!(exact["call_id"], started[0]["call_id"]);
  assert_eq!(exact["host_id"], host);
  assert_eq!(
    exact["original_observation"],
    json!({"snapshot_id":args["snapshot_id"],"revision":args["revision"]})
  );
  let preparation = &exact["preparation"];
  assert_eq!(
    preparation["binding"],
    json!({"daemon_session_id":node.session_id,"run_id":pending.run_id,"owner_epoch":1})
  );
  assert_eq!(preparation["request"]["action_id"], exact["action_id"]);
  assert_eq!(
    preparation["request"]["action"],
    json!({"Click":{"x":10,"y":10,"button":"Left"}})
  );
  assert!(uuid::Uuid::parse_str(preparation["preparation_id"].as_str().unwrap()).is_ok());
  assert_eq!(preparation["content_digest"].as_array().unwrap().len(), 32);
  assert!(preparation["request"]["lease"].is_object());
  assert_eq!(
    node
      .daemon
      .store
      .operation(exact["action_id"].as_str().unwrap())
      .unwrap(),
    None
  );
  exact
}
