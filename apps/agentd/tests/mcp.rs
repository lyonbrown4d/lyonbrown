mod mcp_support;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use mcp_support::{Fixture, Node};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

fn arguments(message: &str) -> Value {
  json!({"call":{"server":"fixture","tool":"echo","arguments":{"message":message}}})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_model_protocol_persists_exact_mcp_approval_and_consumes_only_accepted_calls() {
  for protocol in StandardProtocol::ALL {
    for approved in [false, true] {
      let fixture = Fixture::default();
      let (config, stop) = fixture.start().await;
      let main = MockModelServer::start(vec![
        tool_reply(protocol, "mcp_call", arguments("exact fixture value")),
        text_reply(protocol, "finished"),
      ])
      .await
      .expect("model fixture");
      let node = Node::start(&main, protocol, Some(config)).await;
      let run = node.run("invoke the configured MCP echo tool").await;
      let approval = node.pending(&run.id).await;
      assert_eq!(approval.run_id, run.id);
      assert_eq!(approval.tool, "mcp_call");
      assert_eq!(approval.status, "pending");
      let events = node
        .daemon
        .store
        .events(&run.id, 0, 1000)
        .expect("durable events");
      let tool_started: Value = serde_json::from_str(
        &events
          .iter()
          .find(|event| event.kind == "tool_started")
          .expect("durable tool intent")
          .payload_json,
      )
      .expect("tool intent JSON");
      let exact: Value = serde_json::from_str(&approval.arguments_json).expect("exact approval");
      assert_eq!(
        exact,
        json!({"server":"fixture","tool":"echo","call_id":tool_started["call_id"],"arguments":{"message":"exact fixture value"}})
      );
      assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
      assert_eq!(
        node
          .client
          .get_run(&run.id)
          .await
          .expect("pending run")
          .status,
        "awaiting_approval"
      );

      let plan: Value = serde_json::from_str(
        &events
          .iter()
          .find(|event| event.kind == "execution_plan")
          .expect("persisted effective plan")
          .payload_json,
      )
      .expect("plan JSON");
      assert!(
        plan["allowed_tools"]
          .as_array()
          .expect("allowed tools")
          .contains(&json!("mcp_call"))
      );
      assert_eq!(plan["source"]["kind"], "explicit");
      let requests = main.requests().await;
      let tools = requests[0].body["tools"]
        .as_array()
        .expect("actual Harness tools");
      let schema = match protocol {
        StandardProtocol::ChatCompletions => &tools
          .iter()
          .find(|tool| tool["function"]["name"] == "mcp_call")
          .expect("MCP schema")["function"]["parameters"],
        StandardProtocol::Responses => &tools
          .iter()
          .find(|tool| tool["name"] == "mcp_call")
          .expect("MCP schema")["parameters"],
        StandardProtocol::Anthropic => &tools
          .iter()
          .find(|tool| tool["name"] == "mcp_call")
          .expect("MCP schema")["input_schema"],
      };
      assert_eq!(
        schema["properties"]["call"]["oneOf"][0]["properties"]["arguments"]["properties"]["message"]
          ["type"],
        "string"
      );
      node
        .client
        .approve(&approval.id, approved)
        .await
        .expect("approval decision");
      let terminal = node.terminal(&run.id).await;
      assert_eq!(terminal.status, "completed", "{}", terminal.error);
      let persisted = node
        .daemon
        .store
        .approval(&approval.id)
        .expect("load approval")
        .expect("approval row");
      assert_eq!(
        persisted.status,
        if approved { "consumed" } else { "denied" }
      );
      assert_eq!(fixture.calls.load(Ordering::SeqCst), usize::from(approved));
      if approved {
        assert_eq!(
          fixture.received.lock().await.as_slice(),
          &[json!({"message":"exact fixture value"})]
        );
      }
      assert!(node.client.approve(&approval.id, true).await.is_err());
      let continued = main.requests().await;
      assert_eq!(continued.len(), 2);
      assert!(continued[1].body.to_string().contains(if approved {
        "untrusted_mcp_server"
      } else {
        "operation was not approved"
      }));
      assert!(
        !String::from_utf8(
          node
            .daemon
            .store
            .load_snapshot(&node.session)
            .expect("snapshot")
            .expect("persisted snapshot")
        )
        .expect("snapshot JSON")
        .contains("synthetic-model-secret")
      );
      node.close().await;
      stop.cancel();
    }
  }
}

#[cfg(unix)]
#[tokio::test]
async fn product_configuration_does_not_forward_ambient_provider_credentials_to_stdio() {
  mcp_support::config_process::verify_child_environment_confinement().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn absent_mcp_configuration_exposes_no_mcp_authority_and_executes_no_remote_tool() {
  let protocol = StandardProtocol::ChatCompletions;
  let main = MockModelServer::start(vec![
    tool_reply(protocol, "mcp_call", arguments("forbidden")),
    text_reply(protocol, "safe finish"),
  ])
  .await
  .expect("model fixture");
  let node = Node::start(&main, protocol, None).await;
  let run = node.run("attempt unconfigured MCP tool").await;
  let terminal = node.terminal(&run.id).await;
  assert_eq!(terminal.status, "completed", "{}", terminal.error);
  assert!(
    node
      .client
      .list_approvals(&run.id)
      .await
      .expect("approvals")
      .is_empty()
  );
  let requests = main.requests().await;
  assert!(
    requests[0].body["tools"]
      .as_array()
      .expect("tools")
      .iter()
      .all(|tool| tool["function"]["name"] != "mcp_call")
  );
  let events = node.daemon.store.events(&run.id, 0, 1000).expect("events");
  let plan: Value = serde_json::from_str(
    &events
      .iter()
      .find(|event| event.kind == "execution_plan")
      .expect("effective plan")
      .payload_json,
  )
  .expect("plan");
  assert!(
    !plan["allowed_tools"]
      .as_array()
      .expect("allowed tools")
      .contains(&json!("mcp_call"))
  );
  node.close().await;
}
