mod lifecycle;
mod receipts;
mod runtime;
use crate::support::TestNode;
use agent_protocol::v1;
pub use receipts::assert_receipts;
pub use runtime::{ActionMode, Runtime};
use serde_json::{Value, json};
pub fn route() -> String {
  json!({"task_kind":"browser","model_policy":"general","allowed_tools":["browser_observe","browser_act"],
    "execution_mode":"local","target_hint":null,"risk_level":"high","confidence":0.99}).to_string()
}
pub fn click(tab: &str) -> Value {
  json!({"call":{"op":"click","tab_id":tab,"snapshot_id":"fixture-snapshot-1",
    "revision":1,"element_ref":"submit","timeout_ms":1000}})
}
pub async fn open(
  node: &TestNode,
  runtime: &Runtime,
  session: &str,
) -> (v1::BrowserSpace, v1::BrowserTab) {
  let space = node
    .client
    .open_browser_space_on_host(session, "fixture", false, &runtime.host)
    .await
    .unwrap();
  let tab = node
    .client
    .open_browser_tab(&space.id, "http://127.0.0.1/fixture?private-token=redacted")
    .await
    .unwrap();
  (space, tab)
}
pub async fn approval(node: &TestNode, run: &str) -> v1::Approval {
  tokio::time::timeout(std::time::Duration::from_secs(5), async {
    let mut events = node.client.watch_run(run, 0).await.unwrap();
    loop {
      if let Some(approval) = node.client.list_approvals(run).await.unwrap().first() {
        return approval.clone();
      }
      assert!(
        events.message().await.unwrap().is_some(),
        "run ended before browser approval"
      );
    }
  })
  .await
  .unwrap()
}
pub fn schema(request: &Value, name: &str) -> Value {
  request["tools"]
    .as_array()
    .unwrap()
    .iter()
    .find_map(|tool| {
      let tool = tool.get("function").unwrap_or(tool);
      (tool["name"] == name).then(|| {
        tool
          .get("parameters")
          .or_else(|| tool.get("input_schema"))
          .unwrap()
          .clone()
      })
    })
    .unwrap()
}

/// Find the structured tool result inside each protocol's nested message envelope.
pub fn observation_payload(value: &Value) -> Option<Value> {
  match value {
    Value::String(text) => serde_json::from_str::<Value>(text)
      .ok()
      .filter(|value| value["untrusted_page_content"] == true && value["observation"].is_object()),
    Value::Array(values) => values.iter().find_map(observation_payload),
    Value::Object(values) => values.values().find_map(observation_payload),
    _ => None,
  }
}
