use super::{RawHost, binding, capabilities, desktop, detached, runtime};
use crate::{node, support::TestNode};
use agent_computer::{DesktopCapabilities, DesktopError};
use agent_computer_runtime::{
  Command, Operation, RUNTIME_PROTOCOL_VERSION, Reply, RunBinding, RuntimeError, RuntimeResult,
  attach_host, wire,
};
use agent_protocol::v1;
use std::{sync::atomic::Ordering, time::Duration};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tonic::{Code, Status};
use uuid::Uuid;

const DEADLINE: Duration = Duration::from_secs(3);

fn inspect(
  node: &TestNode,
  binding: &RunBinding,
) -> JoinHandle<Result<DesktopCapabilities, Status>> {
  let daemon = node.daemon.clone();
  let binding = binding.clone();
  tokio::spawn(async move {
    daemon
      .computer_capabilities(binding, CancellationToken::new())
      .await
  })
}

fn assert_running(node: &TestNode, binding: &RunBinding) {
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn busy_host_rejects_second_inspection_and_aborted_caller_releases_attachment() {
  let (node, _mock) = node().await;
  let desktop = desktop(&node);
  let binding = binding(&node, &node.session_id);
  let mut host = RawHost::attach(&node, &desktop).await;
  let pending = inspect(&node, &binding);
  let first = host.next().await;
  let command: Command = wire::decode(&first.payload).unwrap();
  assert!(matches!(command.operation, Operation::Capabilities));
  let second = tokio::time::timeout(
    DEADLINE,
    node
      .daemon
      .computer_capabilities(binding.clone(), CancellationToken::new()),
  )
  .await
  .unwrap()
  .unwrap_err();
  assert_eq!(second.code(), Code::ResourceExhausted);
  assert!(
    tokio::time::timeout(Duration::from_millis(100), host.requests.message())
      .await
      .is_err(),
    "busy rejection must not send a second command"
  );
  pending.abort();
  assert!(
    tokio::time::timeout(DEADLINE, pending)
      .await
      .unwrap()
      .unwrap_err()
      .is_cancelled()
  );
  detached(&node).await;
  let (runtime, calls) = runtime(desktop.clone());
  let mut replacement = tokio::time::timeout(
    DEADLINE,
    attach_host(node.client.clone(), &node.session_id, runtime),
  )
  .await
  .unwrap()
  .unwrap();
  let observed = tokio::time::timeout(
    DEADLINE,
    node
      .daemon
      .computer_capabilities(binding.clone(), CancellationToken::new()),
  )
  .await
  .unwrap()
  .unwrap();
  assert_eq!(observed.identity, desktop.identity);
  assert_eq!(calls.capabilities.load(Ordering::SeqCst), 1);
  assert_eq!(calls.effects.load(Ordering::SeqCst), 0);
  assert_running(&node, &binding);
  tokio::time::timeout(DEADLINE, replacement.shutdown())
    .await
    .unwrap()
    .unwrap();
  tokio::time::timeout(DEADLINE, node.close()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forged_response_binding_identity_or_size_disconnects_without_advancing_run() {
  let (node, _mock) = node().await;
  let desktop = desktop(&node);
  let binding = binding(&node, &node.session_id);
  for corruption in [
    "host",
    "session",
    "request",
    "protocol",
    "identity",
    "oversized",
  ] {
    let mut host = RawHost::attach(&node, &desktop).await;
    let pending = inspect(&node, &binding);
    let request = host.next().await;
    let command: Command = wire::decode(&request.payload).unwrap();
    assert_eq!(command.binding, binding);
    assert!(
      matches!(command.operation, Operation::Capabilities),
      "inspection must never request native input"
    );
    let mut identity = desktop.identity.clone();
    if corruption == "identity" {
      identity.user_session = "another-interactive-user".into();
    }
    let reply: RuntimeResult<Reply> = Ok(Reply::Capabilities(capabilities(identity)));
    let mut response = v1::ComputerRuntimeResponse {
      request_id: request.request_id,
      host_id: host.host.clone(),
      daemon_session_id: node.session_id.clone(),
      protocol_version: RUNTIME_PROTOCOL_VERSION,
      payload: wire::encode(&reply).unwrap(),
    };
    match corruption {
      "host" => response.host_id = Uuid::new_v4().to_string(),
      "session" => response.daemon_session_id = Uuid::new_v4().to_string(),
      "request" => response.request_id = Uuid::new_v4().to_string(),
      "protocol" => response.protocol_version += 1,
      "oversized" => {
        // This valid error reply would otherwise return FailedPrecondition;
        // Unavailable therefore proves rejection at the attachment size bound.
        let oversized: RuntimeResult<Reply> = Err(RuntimeError::Desktop(
          DesktopError::BackendUnavailable("x".repeat(4092)),
        ));
        response.payload = wire::encode(&oversized).unwrap();
        assert_eq!(response.payload.len(), 4097);
      }
      _ => {}
    }
    tokio::time::timeout(DEADLINE, host.sender.send(response))
      .await
      .unwrap()
      .unwrap();
    let error = tokio::time::timeout(DEADLINE, pending)
      .await
      .unwrap()
      .unwrap()
      .unwrap_err();
    assert_eq!(error.code(), Code::Unavailable, "{corruption}");
    detached(&node).await;
    match tokio::time::timeout(DEADLINE, host.requests.message())
      .await
      .unwrap()
    {
      Ok(None) | Err(_) => {}
      Ok(Some(_)) => panic!("{corruption} must disconnect without another native command"),
    }
    assert_running(&node, &binding);
  }
  tokio::time::timeout(DEADLINE, node.close()).await.unwrap();
}
