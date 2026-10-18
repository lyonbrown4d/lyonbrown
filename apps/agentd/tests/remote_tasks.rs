#[allow(dead_code)]
mod peer_support;
#[path = "remote_cases/fencing.rs"]
mod remote_fencing;
mod remote_support;

use agent_protocol::v1::{self, orchestration_service_client::OrchestrationServiceClient};
use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol, text_reply};
use remote_support::{Remote, code};
use tonic::Code;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_master_management_uses_worker_harness_and_requires_acceptance() {
  for protocol in StandardProtocol::ALL {
    let mock = MockModelServer::start(vec![text_reply(protocol, "remote result")])
      .await
      .unwrap();
    let remote = Remote::open(&mock, protocol).await;
    let envelope = remote.envelope();
    let id = envelope.task_id.clone();
    let mut target = remote.target();
    target.fingerprint = "caller fingerprint is not authority".into();
    remote
      .client
      .dispatch_task(target, envelope.clone())
      .await
      .unwrap();
    let task = remote.terminal(&id).await;
    assert_eq!(task.attempt_status, "succeeded", "{}", task.error);
    assert_eq!(task.status, "awaiting_acceptance");
    assert_eq!(task.result, "remote result");
    assert_eq!(
      remote.client.task(&id).await.unwrap().status,
      "awaiting_acceptance"
    );
    assert_eq!(
      remote.pair.master.store.task(&id).unwrap().unwrap().status,
      "awaiting_acceptance"
    );
    let page = remote
      .client
      .list_tasks(&remote.pair.session.id, "", 1)
      .await
      .unwrap();
    assert_eq!(page.tasks.len(), 1);
    assert!(page.next_task_id.is_empty());
    assert_eq!(
      code(
        remote
          .client
          .dispatch_task(remote.target(), envelope)
          .await
          .unwrap_err()
      ),
      Code::AlreadyExists
    );
    assert_eq!(mock.requests().await.len(), 1);
    let accepted = remote
      .client
      .accept_task_result(remote.target(), &id, true)
      .await
      .unwrap();
    assert_eq!(accepted.status, "completed");
    assert_eq!(
      remote.pair.worker.store.task(&id).unwrap().unwrap().status,
      "completed"
    );
    assert_eq!(mock.requests().await[0].path, protocol.path());
    remote.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_scope_pairing_and_authentication_reject_before_dispatch_effects() {
  let mock = MockModelServer::start(vec![]).await.unwrap();
  let remote = Remote::open(&mock, StandardProtocol::ChatCompletions).await;
  let envelope = remote.envelope();
  let mut raw = OrchestrationServiceClient::connect(remote.endpoint.clone())
    .await
    .unwrap();
  let denied = raw
    .dispatch_task(v1::DispatchTaskRequest {
      target: Some(remote.target()),
      envelope: Some(envelope.clone()),
    })
    .await
    .unwrap_err();
  assert_eq!(denied.code(), Code::Unauthenticated);
  let mut malformed = envelope.clone();
  malformed.owner_epoch += 1;
  assert_eq!(
    code(
      remote
        .client
        .dispatch_task(remote.target(), malformed)
        .await
        .unwrap_err()
    ),
    Code::PermissionDenied
  );
  let other = remote
    .pair
    .master
    .store
    .create_session(
      "other",
      &remote.pair.session.workspace,
      &remote.pair.master.node_id,
    )
    .unwrap();
  malformed = envelope.clone();
  malformed.session_id = other.id;
  assert_eq!(
    code(
      remote
        .client
        .dispatch_task(remote.target(), malformed)
        .await
        .unwrap_err()
    ),
    Code::FailedPrecondition
  );
  malformed = envelope.clone();
  malformed.capabilities = vec!["commands.execute".into()];
  assert_eq!(
    code(
      remote
        .client
        .dispatch_task(remote.target(), malformed)
        .await
        .unwrap_err()
    ),
    Code::PermissionDenied
  );
  malformed = envelope.clone();
  malformed.instructions = "x".repeat(64 * 1024 + 1);
  assert_eq!(
    code(
      remote
        .client
        .dispatch_task(remote.target(), malformed)
        .await
        .unwrap_err()
    ),
    Code::InvalidArgument
  );
  let mut target = remote.target();
  target.endpoint = "https://user:secret@localhost/".into();
  assert_eq!(
    code(
      remote
        .client
        .dispatch_task(target, envelope.clone())
        .await
        .unwrap_err()
    ),
    Code::InvalidArgument
  );
  target = remote.target();
  target.id = agent_core::new_id();
  assert_eq!(
    code(
      remote
        .client
        .dispatch_task(target, envelope.clone())
        .await
        .unwrap_err()
    ),
    Code::PermissionDenied
  );
  assert!(
    remote
      .pair
      .master
      .store
      .task(&envelope.task_id)
      .unwrap()
      .is_none()
  );
  assert!(
    remote
      .pair
      .worker
      .store
      .task(&envelope.task_id)
      .unwrap()
      .is_none()
  );
  assert!(mock.requests().await.is_empty());
  remote.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_is_target_bound_and_terminal_parent_cannot_dispatch() {
  let mock = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let remote = Remote::open(&mock, StandardProtocol::ChatCompletions).await;
  let envelope = remote.envelope();
  let id = envelope.task_id.clone();
  remote
    .client
    .dispatch_task(remote.target(), envelope.clone())
    .await
    .unwrap();
  tokio::time::timeout(std::time::Duration::from_secs(5), mock.wait_for_requests(1))
    .await
    .unwrap();
  let mut wrong = remote.target();
  wrong.id = agent_core::new_id();
  assert_eq!(
    code(remote.client.cancel_task(wrong, &id).await.unwrap_err()),
    Code::PermissionDenied
  );
  assert_eq!(
    remote.pair.master.store.task(&id).unwrap().unwrap().status,
    "running"
  );
  assert_eq!(
    remote
      .client
      .cancel_task(remote.target(), &id)
      .await
      .unwrap()
      .status,
    "cancel_requested"
  );
  assert_eq!(remote.terminal(&id).await.status, "cancelled");
  assert_eq!(
    remote
      .pair
      .worker
      .store
      .task(&id)
      .unwrap()
      .unwrap()
      .attempt_status,
    "cancelled"
  );
  let denied = remote.envelope();
  remote
    .pair
    .master
    .store
    .update_run_owned(
      &denied.parent_run_id,
      denied.owner_epoch,
      agent_core::RunStatus::CancelRequested,
      "",
      "",
    )
    .unwrap();
  assert_eq!(
    code(
      remote
        .client
        .dispatch_task(remote.target(), denied.clone())
        .await
        .unwrap_err()
    ),
    Code::FailedPrecondition
  );
  assert!(
    remote
      .pair
      .master
      .store
      .task(&denied.task_id)
      .unwrap()
      .is_none()
  );
  assert!(
    remote
      .pair
      .worker
      .store
      .task(&denied.task_id)
      .unwrap()
      .is_none()
  );
  assert_eq!(mock.requests().await.len(), 1);
  remote.close().await;
}
