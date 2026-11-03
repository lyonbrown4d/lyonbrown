#[allow(dead_code)]
mod support;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply_with_id};
use memory_core::{MemoryRepository, MemoryScope, MemoryState};
use serde_json::json;
use support::TestNode;

fn route() -> String {
  json!({"task_kind":"code","model_policy":"general","allowed_tools":["memory_read","memory_propose"],"execution_mode":"local","target_hint":null,"risk_level":"low","confidence":0.95}).to_string()
}
fn read_output(protocol: StandardProtocol, body: &serde_json::Value) -> Option<String> {
  let value = match protocol {
    StandardProtocol::ChatCompletions => body["messages"]
      .as_array()?
      .iter()
      .find(|message| message["tool_call_id"] == "read_memory")?
      .get("content")?,
    StandardProtocol::Responses => body["input"]
      .as_array()?
      .iter()
      .find(|message| {
        message["call_id"] == "read_memory" && message["type"] == "function_call_output"
      })?
      .get("output")?,
    StandardProtocol::Anthropic => body["messages"]
      .as_array()?
      .iter()
      .filter_map(|message| message["content"].as_array())
      .flatten()
      .find(|part| part["type"] == "tool_result" && part["tool_use_id"] == "read_memory")?
      .get("content")?,
  };
  value.as_str().map(str::to_owned).or_else(|| {
    Some(
      value
        .as_array()?
        .iter()
        .filter_map(|part| part["text"].as_str())
        .collect::<String>(),
    )
  })
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standard_protocol_tools_persist_only_proposals_and_reads_do_not_treat_them_as_active() {
  for protocol in StandardProtocol::ALL {
    let id = agent_core::new_id();
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
      .await
      .unwrap();
    let main = MockModelServer::start(vec![
      tool_reply_with_id(protocol, "propose_memory", "memory_propose", json!({
        "id":id,"expected_revision":0,"scope":"session","kind":"preference","content":"Use synthetic fixture format"
      })),
      tool_reply_with_id(protocol, "read_memory", "memory_read", json!({"id":id})),
      text_reply(protocol, "proposal is awaiting user review"),
    ]).await.unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    let run = node
      .run("propose a synthetic preference for later user review")
      .await;
    let terminal = node.terminal(&run.id).await;
    assert_eq!(terminal.status, "completed", "{}", terminal.error);
    let persisted = node
      .daemon
      .store
      .get(&id)
      .await
      .unwrap()
      .expect("proposal persisted");
    assert_eq!(
      persisted.scope,
      MemoryScope::Session(node.session_id.clone())
    );
    assert_eq!(persisted.state, MemoryState::Proposed);
    assert_eq!(persisted.revision, 1);
    assert_eq!(persisted.content, "Use synthetic fixture format");
    assert!(
      node
        .client
        .list_approvals(&run.id)
        .await
        .unwrap()
        .is_empty()
    );
    let requests = main.requests().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(
      read_output(protocol, &requests[2].body).as_deref(),
      Some("null"),
      "a proposal must not be returned as active"
    );
    let events = node.daemon.store.events(&run.id, 0, 1000).await.unwrap();
    assert!(events.iter().any(|event| event.kind == "tool_started"));
    assert!(events.iter().any(|event| event.kind == "tool_finished"));
    assert!(
      !serde_json::to_string(&events)
        .unwrap()
        .contains("synthetic-main-secret")
    );
    node.close().await;
  }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_cannot_choose_active_state_or_foreign_scope_identifier() {
  let protocol = StandardProtocol::ChatCompletions;
  for extra in [
    json!({"state":"active"}),
    json!({"scope_id":"another-owner"}),
  ] {
    let id = agent_core::new_id();
    let mut arguments =
      json!({"id":id,"scope":"user","kind":"fact","content":"untrusted proposal"});
    arguments
      .as_object_mut()
      .unwrap()
      .extend(extra.as_object().unwrap().clone());
    let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
      .await
      .unwrap();
    let main = MockModelServer::start(vec![
      tool_reply_with_id(protocol, "rejected_proposal", "memory_propose", arguments),
      text_reply(protocol, "rejected"),
    ])
    .await
    .unwrap();
    let node = TestNode::start(&main, &classifier, protocol).await;
    let run = node
      .run("try a proposal with forbidden authority fields")
      .await;
    assert_eq!(node.terminal(&run.id).await.status, "completed");
    assert!(node.daemon.store.get(&id).await.unwrap().is_none());
    assert!(node.daemon.store.jobs(&id).await.unwrap().is_empty());
    assert_eq!(main.requests().await.len(), 2);
    node.close().await;
  }
}
