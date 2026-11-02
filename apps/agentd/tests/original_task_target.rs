#[allow(dead_code)]
mod peer_support;
#[allow(dead_code)]
mod remote_support;

use agent_test_support::{MockModelServer, ScriptedReply, StandardProtocol, text_reply};
use remote_support::{Remote, code};
use tonic::Code;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_route_survives_later_events_and_accepts_without_client_target() {
  let protocol = StandardProtocol::ChatCompletions;
  let mock = MockModelServer::start(vec![text_reply(protocol, "original worker result")])
    .await
    .unwrap();
  let remote = Remote::open(&mock, protocol).await;
  let envelope = remote.envelope().await;
  let id = &envelope.task_id;
  let target = remote.target();
  remote
    .client
    .dispatch_task(target.clone(), envelope.clone())
    .await
    .unwrap();
  assert_eq!(
    remote
      .pair
      .master
      .store
      .task_target_route(id)
      .await
      .unwrap(),
    Some((
      target.id.clone(),
      url::Url::parse(&target.endpoint).unwrap().to_string()
    ))
  );
  assert!(
    !remote
      .pair
      .master
      .store
      .accept_master_dispatched_task_at(
        envelope.clone().into(),
        "another-worker",
        "https://another-worker.invalid/",
      )
      .await
      .unwrap()
      .1
  );
  assert_eq!(
    remote
      .pair
      .master
      .store
      .task_target_route(id)
      .await
      .unwrap(),
    Some((
      target.id,
      url::Url::parse(&target.endpoint).unwrap().to_string()
    ))
  );
  // Subsequent persisted updates may not replace the first dispatch route.
  remote.terminal(id).await;
  let task = remote.client.reconcile_task_original(id).await.unwrap();
  assert_eq!(task.status, "awaiting_acceptance");
  assert_eq!(task.result, "original worker result");
  assert_eq!(task.envelope.as_ref(), Some(&envelope));
  let decided = remote
    .client
    .accept_task_result_original(id, true)
    .await
    .unwrap();
  assert_eq!(decided.status, "completed");
  assert_eq!(
    remote
      .pair
      .master
      .store
      .task(id)
      .await
      .unwrap()
      .unwrap()
      .status,
    "completed"
  );
  assert_eq!(
    remote
      .pair
      .worker
      .store
      .task(id)
      .await
      .unwrap()
      .unwrap()
      .status,
    "completed"
  );
  assert_eq!(mock.requests().await.len(), 1);
  remote.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn old_intent_has_no_inferred_route_and_original_operations_preserve_state() {
  let mock = MockModelServer::start(vec![]).await.unwrap();
  let remote = Remote::open(&mock, StandardProtocol::Responses).await;
  let envelope = remote.envelope().await;
  let id = &envelope.task_id;
  let (before, _) = remote
    .pair
    .master
    .store
    .accept_master_dispatched_task(envelope.clone().into(), &remote.target().id)
    .await
    .unwrap();
  assert_eq!(
    remote
      .pair
      .master
      .store
      .task_target_route(id)
      .await
      .unwrap(),
    None
  );
  for error in [
    remote.client.reconcile_task_original(id).await.unwrap_err(),
    remote.client.cancel_task_original(id).await.unwrap_err(),
    remote
      .client
      .accept_task_result_original(id, true)
      .await
      .unwrap_err(),
  ] {
    assert_eq!(code(error), Code::FailedPrecondition);
  }
  assert_eq!(
    remote.pair.master.store.task(id).await.unwrap().unwrap(),
    before
  );
  assert!(remote.pair.worker.store.task(id).await.unwrap().is_none());
  assert!(mock.requests().await.is_empty());
  remote.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_route_does_not_authorize_unpaired_revoked_or_stale_owner() {
  for rejection in ["unpaired", "revoked", "epoch"] {
    let mock = MockModelServer::start(vec![]).await.unwrap();
    let remote = Remote::open(&mock, StandardProtocol::Responses).await;
    let envelope = remote.envelope().await;
    let id = &envelope.task_id;
    let worker = if rejection == "unpaired" {
      agent_core::new_id()
    } else {
      remote.target().id
    };
    let (before, _) = remote
      .pair
      .master
      .store
      .accept_master_dispatched_task_at(envelope.clone().into(), &worker, &remote.target().endpoint)
      .await
      .unwrap();
    match rejection {
      "revoked" => {
        remote
          .pair
          .master_manager
          .revoke_local(&worker)
          .await
          .unwrap();
      }
      "epoch" => {
        remote
          .pair
          .master
          .store
          .change_owner(
            &remote.pair.session.id,
            remote.pair.session.owner_epoch,
            &remote.pair.master.node_id,
          )
          .await
          .unwrap();
      }
      _ => {}
    }
    for error in [
      remote.client.reconcile_task_original(id).await.unwrap_err(),
      remote.client.cancel_task_original(id).await.unwrap_err(),
      remote
        .client
        .accept_task_result_original(id, true)
        .await
        .unwrap_err(),
    ] {
      assert_eq!(code(error), Code::PermissionDenied, "{rejection}");
    }
    assert_eq!(
      remote.pair.master.store.task(id).await.unwrap().unwrap(),
      before
    );
    assert!(remote.pair.worker.store.task(id).await.unwrap().is_none());
    assert!(mock.requests().await.is_empty());
    remote.close().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn original_cancel_targets_same_attempt_and_lost_peer_never_redispatches() {
  for disconnect in [false, true] {
    let mock = MockModelServer::start(vec![ScriptedReply::stall()])
      .await
      .unwrap();
    let mut remote = Remote::open(&mock, StandardProtocol::ChatCompletions).await;
    let envelope = remote.envelope().await;
    let id = &envelope.task_id;
    remote
      .client
      .dispatch_task(remote.target(), envelope.clone())
      .await
      .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), mock.wait_for_requests(1))
      .await
      .unwrap();
    if disconnect {
      remote.pair.abort_worker_listener().await;
      assert_eq!(
        code(remote.client.cancel_task_original(id).await.unwrap_err()),
        Code::FailedPrecondition
      );
      assert_eq!(
        code(remote.client.reconcile_task_original(id).await.unwrap_err()),
        Code::FailedPrecondition
      );
      assert_eq!(
        remote.client.task(id).await.unwrap().attempt_status,
        "uncertain"
      );
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
    } else {
      assert_eq!(
        remote.client.cancel_task_original(id).await.unwrap().status,
        "cancel_requested"
      );
      assert_eq!(remote.terminal(id).await.attempt_status, "cancelled");
    }
    let task = remote.pair.master.store.task(id).await.unwrap().unwrap();
    assert_eq!(task.envelope, envelope.clone().into());
    assert_eq!(remote.pair.worker.store.tasks().await.unwrap().len(), 1);
    assert_eq!(mock.requests().await.len(), 1);
    remote.close().await;
  }
}
