use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol, text_reply};
use tonic::Code;

use crate::remote_support::{Remote, code};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn task_pages_are_bounded_and_old_master_epoch_cannot_manage_results() {
  let protocol = StandardProtocol::Responses;
  let mock = MockModelServer::start(vec![
    text_reply(protocol, "one"),
    text_reply(protocol, "two"),
  ])
  .await
  .unwrap();
  let remote = Remote::open(&mock, protocol).await;
  let first = remote.envelope();
  let second = remote.envelope();
  for envelope in [&first, &second] {
    remote
      .client
      .dispatch_task(remote.target(), envelope.clone())
      .await
      .unwrap();
    assert_eq!(
      remote.terminal(&envelope.task_id).await.status,
      "awaiting_acceptance"
    );
  }
  let page = remote
    .client
    .list_tasks(&remote.pair.session.id, "", 1)
    .await
    .unwrap();
  assert_eq!(page.tasks.len(), 1);
  assert_eq!(
    page.next_task_id,
    page.tasks[0].envelope.as_ref().unwrap().task_id
  );
  let next = remote
    .client
    .list_tasks(&remote.pair.session.id, &page.next_task_id, 1)
    .await
    .unwrap();
  assert_eq!(next.tasks.len(), 1);
  assert!(next.next_task_id.is_empty());
  assert_ne!(page.tasks[0].envelope, next.tasks[0].envelope);
  assert_eq!(
    code(
      remote
        .client
        .list_tasks(&remote.pair.session.id, "", 501)
        .await
        .unwrap_err()
    ),
    Code::InvalidArgument
  );
  remote
    .pair
    .master
    .store
    .change_owner(
      &remote.pair.session.id,
      remote.pair.session.owner_epoch,
      &remote.pair.master.node_id,
    )
    .unwrap();
  assert_eq!(
    code(remote.client.task(&first.task_id).await.unwrap_err()),
    Code::PermissionDenied
  );
  assert_eq!(
    code(
      remote
        .client
        .reconcile_task(remote.target(), &first.task_id)
        .await
        .unwrap_err()
    ),
    Code::PermissionDenied
  );
  assert_eq!(
    code(
      remote
        .client
        .cancel_task(remote.target(), &first.task_id)
        .await
        .unwrap_err()
    ),
    Code::PermissionDenied
  );
  assert_eq!(
    code(
      remote
        .client
        .accept_task_result(remote.target(), &first.task_id, true)
        .await
        .unwrap_err()
    ),
    Code::PermissionDenied
  );
  assert!(
    remote
      .client
      .list_tasks(&remote.pair.session.id, "", 100)
      .await
      .unwrap()
      .tasks
      .is_empty()
  );
  assert_eq!(
    remote
      .pair
      .master
      .store
      .task(&first.task_id)
      .unwrap()
      .unwrap()
      .status,
    "awaiting_acceptance"
  );
  assert_eq!(mock.requests().await.len(), 2);
  remote.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_cancellation_response_becomes_uncertain_without_replay() {
  let mock = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let mut remote = Remote::open(&mock, StandardProtocol::ChatCompletions).await;
  let envelope = remote.envelope();
  remote
    .client
    .dispatch_task(remote.target(), envelope.clone())
    .await
    .unwrap();
  tokio::time::timeout(std::time::Duration::from_secs(5), mock.wait_for_requests(1))
    .await
    .unwrap();
  remote.pair.abort_worker_listener().await;
  assert_eq!(
    code(
      remote
        .client
        .cancel_task(remote.target(), &envelope.task_id)
        .await
        .unwrap_err()
    ),
    Code::FailedPrecondition
  );
  let task = remote.client.task(&envelope.task_id).await.unwrap();
  assert_eq!(task.status, "uncertain");
  assert_eq!(task.attempt_status, "uncertain");
  assert_eq!(
    code(
      remote
        .client
        .dispatch_task(remote.target(), envelope.clone())
        .await
        .unwrap_err()
    ),
    Code::AlreadyExists
  );
  assert_eq!(mock.requests().await.len(), 1);
  remote.close().await;
}
