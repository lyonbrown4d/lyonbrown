mod computer_support;
#[allow(dead_code)]
mod support;

use agent_computer_runtime::{RUNTIME_PROTOCOL_VERSION, Reply, RuntimeResult, attach_host, wire};
use agent_protocol::v1;
use agent_test_support::{MockModelServer, StandardProtocol};
use computer_support::*;
use std::sync::atomic::Ordering;
use support::TestNode;
use tokio_util::sync::CancellationToken;
use tonic::Code;

async fn node() -> (TestNode, MockModelServer) {
  let mock = MockModelServer::start(vec![]).await.unwrap();
  let node = TestNode::start(&mock, &mock, StandardProtocol::ALL[0]).await;
  (node, mock)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capabilities_require_active_exact_run_and_owner_without_native_effects() {
  let (node, mock) = node().await;
  let desktop = desktop(&node);
  let (runtime, calls) = runtime(desktop.clone());
  let mut host = attach_host(node.client.clone(), &node.session_id, runtime)
    .await
    .unwrap();
  let binding = binding(&node, &node.session_id);
  let observed = node
    .daemon
    .computer_capabilities(binding.clone(), CancellationToken::new())
    .await
    .unwrap();
  assert_eq!(observed.identity, desktop.identity);
  assert_eq!(calls.capabilities.load(Ordering::SeqCst), 1);
  node
    .daemon
    .store
    .update_run_owned(
      &binding.run_id,
      1,
      agent_core::RunStatus::AwaitingApproval,
      "",
      "",
    )
    .unwrap();
  node
    .daemon
    .computer_capabilities(binding.clone(), CancellationToken::new())
    .await
    .unwrap();
  assert_eq!(calls.capabilities.load(Ordering::SeqCst), 2);
  let other = node
    .client
    .create_session("other run", node.workspace.path().to_str().unwrap())
    .await
    .unwrap();
  let mut cross_run = computer_support::binding(&node, &other.id);
  cross_run.daemon_session_id = node.session_id.clone();
  assert_eq!(
    node
      .daemon
      .computer_capabilities(cross_run, CancellationToken::new())
      .await
      .unwrap_err()
      .code(),
    Code::FailedPrecondition
  );
  node
    .daemon
    .store
    .update_run_owned(
      &binding.run_id,
      1,
      agent_core::RunStatus::Completed,
      "done",
      "",
    )
    .unwrap();
  assert_eq!(
    node
      .daemon
      .computer_capabilities(binding.clone(), CancellationToken::new())
      .await
      .unwrap_err()
      .code(),
    Code::FailedPrecondition
  );
  let active = computer_support::binding(&node, &node.session_id);
  node
    .daemon
    .store
    .change_owner(&node.session_id, 1, &node.daemon.node_id)
    .unwrap();
  assert_eq!(
    node
      .daemon
      .computer_capabilities(active.clone(), CancellationToken::new())
      .await
      .unwrap_err()
      .code(),
    Code::FailedPrecondition
  );
  node
    .daemon
    .store
    .change_owner(&node.session_id, 2, "other-owner")
    .unwrap();
  assert_eq!(
    node
      .daemon
      .computer_capabilities(active, CancellationToken::new())
      .await
      .unwrap_err()
      .code(),
    Code::PermissionDenied
  );
  assert_eq!(calls.capabilities.load(Ordering::SeqCst), 2);
  assert_eq!(calls.effects.load(Ordering::SeqCst), 0);
  assert_eq!(
    node
      .daemon
      .store
      .run(&binding.run_id)
      .unwrap()
      .unwrap()
      .status,
    "completed"
  );
  assert!(mock.requests().await.is_empty());
  host.shutdown().await.unwrap();
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_eof_and_invalid_reply_remove_host_and_allow_explicit_replacement() {
  let (node, _mock) = node().await;
  let desktop = desktop(&node);
  let binding = binding(&node, &node.session_id);
  for mode in ["cancel", "eof", "invalid"] {
    let mut host = RawHost::attach(&node, &desktop).await;
    let cancellation = CancellationToken::new();
    let daemon = node.daemon.clone();
    let request_binding = binding.clone();
    let token = cancellation.clone();
    let pending =
      tokio::spawn(async move { daemon.computer_capabilities(request_binding, token).await });
    let request = host.next().await;
    match mode {
      "cancel" => cancellation.cancel(),
      "invalid" => host
        .sender
        .send(v1::ComputerRuntimeResponse {
          request_id: request.request_id,
          host_id: host.host.clone(),
          daemon_session_id: node.session_id.clone(),
          protocol_version: RUNTIME_PROTOCOL_VERSION,
          payload: vec![255],
        })
        .await
        .unwrap(),
      _ => {
        drop(host.sender);
      }
    }
    let error = tokio::time::timeout(std::time::Duration::from_secs(3), pending)
      .await
      .unwrap()
      .unwrap()
      .unwrap_err();
    assert_eq!(
      error.code(),
      if mode == "cancel" {
        Code::Cancelled
      } else {
        Code::Unavailable
      }
    );
    detached(&node).await;
    let (runtime, calls) = runtime(desktop.clone());
    let mut replacement = attach_host(node.client.clone(), &node.session_id, runtime)
      .await
      .unwrap();
    let result = node
      .daemon
      .computer_capabilities(binding.clone(), CancellationToken::new())
      .await
      .unwrap();
    assert_eq!(result.identity, desktop.identity);
    assert_eq!(calls.capabilities.load(Ordering::SeqCst), 1);
    assert_eq!(calls.effects.load(Ordering::SeqCst), 0);
    assert_eq!(
      node
        .daemon
        .computer_runtime_info(&node.session_id)
        .await
        .unwrap()
        .unwrap()
        .host_id,
      replacement.host_id
    );
    replacement.shutdown().await.unwrap();
    detached(&node).await;
  }
  assert_eq!(
    node
      .daemon
      .store
      .run(&binding.run_id)
      .unwrap()
      .unwrap()
      .status,
    "running"
  );
  node.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_change_during_inspection_rejects_reply_and_preserves_run_state() {
  let (node, _mock) = node().await;
  let desktop = desktop(&node);
  let mut host = RawHost::attach(&node, &desktop).await;
  let binding = binding(&node, &node.session_id);
  let daemon = node.daemon.clone();
  let active = binding.clone();
  let pending = tokio::spawn(async move {
    daemon
      .computer_capabilities(active, CancellationToken::new())
      .await
  });
  let request = host.next().await;
  node
    .daemon
    .store
    .change_owner(&node.session_id, 1, "other-owner")
    .unwrap();
  let reply: RuntimeResult<Reply> = Ok(Reply::Capabilities(capabilities(desktop.identity)));
  host
    .sender
    .send(v1::ComputerRuntimeResponse {
      request_id: request.request_id,
      host_id: host.host,
      daemon_session_id: node.session_id.clone(),
      protocol_version: RUNTIME_PROTOCOL_VERSION,
      payload: wire::encode(&reply).unwrap(),
    })
    .await
    .unwrap();
  assert_eq!(
    tokio::time::timeout(std::time::Duration::from_secs(3), pending)
      .await
      .unwrap()
      .unwrap()
      .unwrap_err()
      .code(),
    Code::PermissionDenied
  );
  assert_eq!(
    node
      .daemon
      .store
      .run(&binding.run_id)
      .unwrap()
      .unwrap()
      .status,
    "running"
  );
  node.close().await;
}
