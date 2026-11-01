#[path = "peer_cases/local_models.rs"]
mod local_model_worker;
mod local_support;
mod peer_support;

use std::time::Duration;

use agent_protocol::v1::{self, client_service_server::ClientService};
use agent_test_support::{
  MockModelServer, ScriptedReply, StandardProtocol, text_reply, tool_reply,
};
use peer_support::Pair;
use serde_json::json;
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_daemons_use_real_worker_harness_with_isolated_session_and_capability_reduction() {
  for protocol in StandardProtocol::ALL {
    let mock = MockModelServer::start(vec![
      tool_reply(protocol, "read_file", json!({"path":"master-only.txt"})),
      text_reply(protocol, "worker-independent-result"),
      text_reply(protocol, "no tools result"),
    ])
    .await
    .unwrap();
    let mut pair = Pair::open(&mock, protocol).await;
    pair.serve().await;
    let envelope = pair.envelope(vec!["workspace.read".into()]);
    pair
      .dispatch
      .submit(&pair.worker_manager.local_node(), envelope.clone())
      .await
      .unwrap();
    let task = pair.terminal(&envelope.task_id).await;
    assert_eq!(task.attempt_status, "succeeded", "{}", task.error);
    assert_eq!(task.status, "awaiting_acceptance");
    assert_eq!(task.result, "worker-independent-result");
    let worker_task = pair.worker.store.task(&envelope.task_id).unwrap().unwrap();
    assert_eq!(worker_task.status, "awaiting_acceptance");
    let local_id = format!("worker:{}:{}", envelope.task_id, envelope.attempt_id);
    let session = pair.worker.store.session(&local_id).unwrap().unwrap();
    assert_ne!(session.id, pair.session.id);
    assert_ne!(session.workspace, envelope.workspace);
    assert_eq!(session.owner_node_id, pair.worker.node_id);
    assert!(
      !std::path::Path::new(&session.workspace)
        .join("master-only.txt")
        .exists()
    );
    let runs = pair.worker.store.runs(&session.id).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, "completed");
    let events = pair.worker.store.events(&runs[0].id, 0, 1000).unwrap();
    assert!(events.iter().any(|event| event.kind == "run_started"));
    let plan: serde_json::Value = serde_json::from_str(
      &events
        .iter()
        .find(|event| event.kind == "execution_plan")
        .unwrap()
        .payload_json,
    )
    .unwrap();
    assert_eq!(
      plan["allowed_tools"],
      json!(["read_file", "list_files", "search_files"])
    );
    let requests = mock.requests().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, protocol.path());
    assert_eq!(requests[0].body["model"], "worker-fixture");
    assert_eq!(requests[0].body["tools"].as_array().unwrap().len(), 3);
    let mut names = requests[0].body["tools"]
      .as_array()
      .unwrap()
      .iter()
      .map(|tool| {
        tool.get("function").unwrap_or(tool)["name"]
          .as_str()
          .unwrap()
      })
      .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(names, ["list_files", "read_file", "search_files"]);
    assert!(
      !requests[1]
        .body
        .to_string()
        .contains("master-private-content")
    );
    assert!(
      pair
        .dispatch
        .submit(&pair.worker_manager.local_node(), envelope.clone())
        .await
        .is_err()
    );
    assert_eq!(mock.requests().await.len(), 2);
    assert_eq!(pair.worker.store.runs(&session.id).unwrap().len(), 1);
    let accepted = pair
      .dispatch
      .accept(&pair.worker_manager.local_node(), &envelope.task_id, true)
      .await
      .unwrap();
    assert_eq!(accepted.status, "completed");
    assert_eq!(
      pair
        .worker
        .store
        .task(&envelope.task_id)
        .unwrap()
        .unwrap()
        .status,
      "completed"
    );

    let empty = pair.envelope(vec![]);
    pair
      .dispatch
      .submit(&pair.worker_manager.local_node(), empty.clone())
      .await
      .unwrap();
    assert_eq!(
      pair.terminal(&empty.task_id).await.attempt_status,
      "succeeded"
    );
    let requests = mock.requests().await;
    assert_eq!(requests.len(), 3);
    assert!(
      requests[2]
        .body
        .get("tools")
        .is_none_or(|tools| tools.as_array().is_some_and(Vec::is_empty))
    );
    let empty_runs = pair.worker_runs(&empty);
    let empty_events = pair
      .worker
      .store
      .events(&empty_runs[0].id, 0, 1000)
      .unwrap();
    let plan: serde_json::Value = serde_json::from_str(
      &empty_events
        .iter()
        .find(|event| event.kind == "execution_plan")
        .unwrap()
        .payload_json,
    )
    .unwrap();
    assert_eq!(plan["allowed_tools"], json!([]));
    pair.stop().await;
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_tasks_participate_in_upgrade_fence_and_shutdown_cancellation() {
  let mock = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let mut pair = Pair::open(&mock, StandardProtocol::ChatCompletions).await;
  pair.serve().await;
  let envelope = pair.envelope(vec![]);
  pair
    .dispatch
    .submit(&pair.worker_manager.local_node(), envelope.clone())
    .await
    .unwrap();
  tokio::time::timeout(Duration::from_secs(5), mock.wait_for_requests(1))
    .await
    .unwrap();
  let node = pair
    .worker
    .prepare_upgrade(Request::new(v1::Empty {}))
    .await
    .unwrap()
    .into_inner();
  assert!(node.draining);
  assert_eq!(node.active_runs, 1);
  let denied = pair.envelope(vec![]);
  assert!(
    pair
      .dispatch
      .submit(&pair.worker_manager.local_node(), denied.clone())
      .await
      .is_err()
  );
  assert!(pair.worker.store.task(&denied.task_id).unwrap().is_none());
  pair.worker.shutdown().await;
  let task = pair.worker.store.task(&envelope.task_id).unwrap().unwrap();
  assert_eq!(task.attempt_status, "cancelled");
  let runs = pair.worker_runs(&envelope);
  assert_eq!(runs[0].status, "cancelled");
  assert_eq!(
    pair
      .worker
      .get_node(Request::new(v1::Empty {}))
      .await
      .unwrap()
      .into_inner()
      .active_runs,
    0
  );
  assert_eq!(mock.requests().await.len(), 1);
  pair.stop().await;
}

#[tokio::test]
async fn interrupted_worker_task_and_pending_run_are_uncertain_without_model_replay() {
  let mock = MockModelServer::start(vec![]).await.unwrap();
  let pair = Pair::open(&mock, StandardProtocol::ChatCompletions).await;
  let envelope = pair.envelope(vec![]);
  let workspace = pair
    .worker
    .config
    .data_path()
    .unwrap()
    .join("worker-workspaces")
    .join(&envelope.task_id)
    .join(&envelope.attempt_id);
  let (_, session, _) = pair
    .worker
    .store
    .accept_worker_task(
      envelope.clone(),
      &pair.worker.node_id,
      workspace.to_str().unwrap(),
    )
    .unwrap();
  let (run, _) = pair
    .worker
    .store
    .accept_run(
      &session.id,
      session.owner_epoch,
      &envelope.instructions,
      &envelope.attempt_id,
    )
    .unwrap();
  pair.worker.recover().await.unwrap();
  let runtime = agentd::peers::PeerRuntime::prepare(pair.worker.clone())
    .await
    .unwrap()
    .unwrap();
  assert_eq!(
    pair
      .worker
      .store
      .task(&envelope.task_id)
      .unwrap()
      .unwrap()
      .attempt_status,
    "uncertain"
  );
  assert_eq!(
    pair.worker.store.run(&run.id).unwrap().unwrap().status,
    "uncertain"
  );
  assert!(mock.requests().await.is_empty());
  drop(runtime);
  pair.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_peer_listener_does_not_orphan_accepted_worker_execution() {
  let mock = MockModelServer::start(vec![ScriptedReply::stall()])
    .await
    .unwrap();
  let mut pair = Pair::open(&mock, StandardProtocol::ChatCompletions).await;
  pair.serve().await;
  let envelope = pair.envelope(vec![]);
  pair
    .dispatch
    .submit(&pair.worker_manager.local_node(), envelope.clone())
    .await
    .unwrap();
  tokio::time::timeout(Duration::from_secs(5), mock.wait_for_requests(1))
    .await
    .unwrap();
  pair.abort_worker_listener().await;
  let snapshot = pair
    .worker
    .get_node(Request::new(v1::Empty {}))
    .await
    .unwrap()
    .into_inner();
  assert_eq!(snapshot.active_runs, 1);
  pair.worker.shutdown().await;
  assert_eq!(
    pair
      .worker
      .store
      .task(&envelope.task_id)
      .unwrap()
      .unwrap()
      .attempt_status,
    "cancelled"
  );
  let runs = pair.worker_runs(&envelope);
  assert_eq!(runs[0].status, "cancelled");
  assert_eq!(mock.requests().await.len(), 1);
  pair.stop().await;
}
