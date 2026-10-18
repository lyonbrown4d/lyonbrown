use super::{Fixture, Node};
use agent_client::AgentClient;
use agent_test_support::{MockModelServer, StandardProtocol, text_reply, tool_reply};
use serde_json::json;
use std::sync::atomic::Ordering;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_mcp_connections_keep_approval_authority_bound_to_each_authenticated_run() {
  let protocol = StandardProtocol::ChatCompletions;
  let fixture = Fixture::default();
  let (config, stop) = fixture.start().await;
  let arguments =
    |message| json!({"call":{"server":"fixture","tool":"echo","arguments":{"message":message}}});
  let main = MockModelServer::start(vec![
    tool_reply(protocol, "mcp_call", arguments("first session")),
    tool_reply(protocol, "mcp_call", arguments("second session")),
    text_reply(protocol, "first done"),
    text_reply(protocol, "second done"),
  ])
  .await
  .expect("model fixture");
  let node = Node::start(&main, protocol, Some(config)).await;
  let first = node.run("first MCP request").await;
  let first_approval = node.pending(&first.id).await;
  let workspace = node
    .daemon
    .store
    .session(&node.session)
    .expect("session row")
    .expect("first session")
    .workspace;
  let second_session = node
    .client
    .create_session("second session", &workspace)
    .await
    .expect("second session");
  let second = node
    .client
    .start_run(
      &second_session.id,
      "second MCP request",
      &agent_core::new_id(),
    )
    .await
    .expect("second run");
  let second_approval = node.pending(&second.id).await;
  assert_ne!(first_approval.run_id, second_approval.run_id);
  assert!(
    AgentClient::connect_authenticated(
      &format!("http://{}", node.daemon.config.listen),
      "wrong-fixture-token"
    )
    .await
    .is_err()
  );
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
  assert_eq!(
    node
      .daemon
      .store
      .approval(&first_approval.id)
      .expect("approval")
      .expect("approval row")
      .status,
    "pending"
  );
  node
    .client
    .approve(&first_approval.id, true)
    .await
    .expect("approve first run");
  assert_eq!(node.terminal(&first.id).await.status, "completed");
  assert_eq!(
    node
      .client
      .get_run(&second.id)
      .await
      .expect("second run status")
      .status,
    "awaiting_approval"
  );
  assert_eq!(
    node
      .daemon
      .store
      .approval(&second_approval.id)
      .expect("second approval")
      .expect("second approval row")
      .status,
    "pending"
  );
  assert_eq!(
    fixture.received.lock().await.as_slice(),
    &[json!({"message":"first session"})]
  );
  node
    .client
    .approve(&second_approval.id, false)
    .await
    .expect("deny second run");
  assert_eq!(node.terminal(&second.id).await.status, "completed");
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
  node.close().await;
  stop.cancel();
}
