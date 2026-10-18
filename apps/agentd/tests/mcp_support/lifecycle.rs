use super::{Fixture, Node};
use crate::arguments;
use agent_test_support::{MockModelServer, StandardProtocol, tool_reply};
use std::{sync::atomic::Ordering, time::Duration};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_after_approved_dispatch_is_durable_uncertain_and_never_replayed() {
  let protocol = StandardProtocol::ChatCompletions;
  let fixture = Fixture::default();
  let (config, stop) = fixture.start().await;
  let main = MockModelServer::start(vec![tool_reply(protocol, "mcp_call", arguments("wait"))])
    .await
    .expect("model fixture");
  let node = Node::start(&main, protocol, Some(config)).await;
  let run = node.run("wait in MCP tool").await;
  let approval = node.pending(&run.id).await;
  node
    .client
    .approve(&approval.id, true)
    .await
    .expect("approve");
  tokio::time::timeout(Duration::from_secs(3), fixture.started.notified())
    .await
    .expect("MCP actual dispatch");
  node.client.cancel_run(&run.id).await.expect("cancel run");
  let terminal = node.terminal(&run.id).await;
  assert_eq!(terminal.status, "uncertain", "{}", terminal.error);
  assert_eq!(
    node
      .client
      .cancel_run(&run.id)
      .await
      .expect("idempotent uncertain cancellation")
      .status,
    "uncertain"
  );
  assert_eq!(
    node
      .daemon
      .store
      .run(&run.id)
      .expect("run row")
      .expect("persisted run")
      .status,
    "uncertain"
  );
  assert_eq!(
    node
      .daemon
      .store
      .approval(&approval.id)
      .expect("approval row")
      .expect("persisted approval")
      .status,
    "consumed"
  );
  let rejected = node
    .client
    .start_run(
      &node.session,
      "do not replay uncertain MCP call",
      &agent_core::new_id(),
    )
    .await;
  assert!(
    rejected.is_err(),
    "unresolved execution must fence this session"
  );
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
  assert_eq!(main.requests().await.len(), 1);
  assert_eq!(
    node
      .client
      .list_runs(&node.session)
      .await
      .expect("persisted runs")
      .len(),
    1
  );
  node.close().await;
  stop.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_pending_mcp_approval_never_dispatches_and_cannot_be_approved_later() {
  let protocol = StandardProtocol::ChatCompletions;
  let fixture = Fixture::default();
  let (config, stop) = fixture.start().await;
  let main = MockModelServer::start(vec![tool_reply(
    protocol,
    "mcp_call",
    arguments("not yet approved"),
  )])
  .await
  .expect("model fixture");
  let node = Node::start(&main, protocol, Some(config)).await;
  let run = node.run("cancel pending MCP approval").await;
  let approval = node.pending(&run.id).await;
  node
    .client
    .cancel_run(&run.id)
    .await
    .expect("cancel pending run");
  let terminal = node.terminal(&run.id).await;
  assert_eq!(terminal.status, "cancelled", "{}", terminal.error);
  assert!(node.client.approve(&approval.id, true).await.is_err());
  assert_eq!(
    node
      .daemon
      .store
      .approval(&approval.id)
      .expect("approval row")
      .expect("closed approval")
      .status,
    "cancelled"
  );
  assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
  assert_eq!(main.requests().await.len(), 1);
  node.close().await;
  stop.cancel();
}
