//! Hold an authenticated inspection until an approved input has committed intent.
use super::*;
use agent_computer_runtime::{
  Command, Operation, RUNTIME_PROTOCOL_VERSION, RunBinding, RuntimeHello, wire,
};
use agent_protocol::v1;
use agent_test_support::{StandardProtocol, text_reply, tool_reply_with_id};
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};
use tokio::sync::{Notify, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

struct HeldInspection {
  host_id: String,
  hold: Arc<AtomicBool>,
  entered: Arc<Notify>,
  release: Arc<Notify>,
  task: tokio::task::JoinHandle<()>,
}
impl HeldInspection {
  async fn attach(node: &TestNode, runtime: &Runtime) -> Self {
    let host_id = uuid::Uuid::new_v4().to_string();
    let session = node.session_id.clone();
    let (sender, receiver) = mpsc::channel(2);
    sender
      .send(v1::ComputerRuntimeResponse {
        request_id: String::new(),
        host_id: host_id.clone(),
        daemon_session_id: session.clone(),
        protocol_version: RUNTIME_PROTOCOL_VERSION,
        payload: wire::encode(&RuntimeHello {
          desktop_session: runtime.engine.session().clone(),
        })
        .unwrap(),
      })
      .await
      .unwrap();
    let mut requests = node
      .client
      .attach_computer_runtime(ReceiverStream::new(receiver))
      .await
      .unwrap();
    let hold = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (armed, blocked, resume) = (hold.clone(), entered.clone(), release.clone());
    let host = host_id.clone();
    let engine = runtime.engine.clone();
    let task = tokio::spawn(async move {
      while let Ok(Some(request)) = requests.message().await {
        let command: Command = wire::decode(&request.payload).unwrap();
        let should_hold = matches!(command.operation, Operation::Capabilities)
          && armed.swap(false, Ordering::SeqCst);
        let engine = engine.clone();
        let result =
          tokio::task::spawn_blocking(move || engine.handle(command, CancellationToken::new()))
            .await
            .unwrap();
        if should_hold {
          blocked.notify_one();
          tokio::select! {
            _ = resume.notified() => {},
            _ = requests.message() => panic!("host must never receive overlapping dispatch"),
          }
        }
        if sender
          .send(v1::ComputerRuntimeResponse {
            request_id: request.request_id,
            host_id: host.clone(),
            daemon_session_id: session.clone(),
            protocol_version: RUNTIME_PROTOCOL_VERSION,
            payload: wire::encode(&result).unwrap(),
          })
          .await
          .is_err()
        {
          return;
        }
      }
    });
    Self {
      host_id,
      hold,
      entered,
      release,
      task,
    }
  }
}
impl Drop for HeldInspection {
  fn drop(&mut self) {
    self.task.abort();
  }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn approved_execute_waits_for_host_inspection_then_completes_once() {
  let protocol = StandardProtocol::ChatCompletions;
  let classifier = MockModelServer::start(vec![text_reply(protocol, &route())])
    .await
    .unwrap();
  let model = MockModelServer::start_interactive().await.unwrap();
  let node = TestNode::start_configured(&model, &classifier, protocol, |config| {
    config.computer_tools = true
  })
  .await;
  let runtime = Runtime::new(&node);
  let host = HeldInspection::attach(&node, &runtime).await;
  let run = node
    .run("Serialize approved input behind another host command")
    .await;
  model
    .push_reply(tool_reply_with_id(
      protocol,
      "observe-computer",
      "computer_observe",
      json!({"screenshot":false}),
    ))
    .await;
  let args = click(&observation(&model).await);
  model
    .push_reply(tool_reply_with_id(
      protocol,
      "act-computer",
      "computer_act",
      args.clone(),
    ))
    .await;
  let pending = approval(&node, &run.id).await;
  let exact = exact_approval(&node, &pending, &host.host_id, &args);
  host.hold.store(true, Ordering::SeqCst);
  let daemon = node.daemon.clone();
  let binding = RunBinding {
    daemon_session_id: node.session_id.clone(),
    run_id: run.id.clone(),
    owner_epoch: 1,
  };
  let inspection = tokio::spawn(async move {
    daemon
      .computer_capabilities(binding, CancellationToken::new())
      .await
  });
  tokio::time::timeout(Duration::from_secs(5), host.entered.notified())
    .await
    .unwrap();
  node.client.approve(&pending.id, true).await.unwrap();
  let action = exact["action_id"].as_str().unwrap();
  tokio::time::timeout(Duration::from_secs(5), async {
    let mut events = node.client.watch_run(&run.id, 0).await.unwrap();
    loop {
      if node.daemon.store.operation(action).unwrap().is_some() {
        break;
      }
      assert!(events.message().await.unwrap().is_some());
    }
  })
  .await
  .unwrap();
  // The held request prevents native dispatch even after approval is consumed.
  // Releasing it must produce a successful single effect and completed receipt.
  assert_eq!(
    node
      .daemon
      .store
      .operation(action)
      .unwrap()
      .unwrap()
      .status
      .as_str(),
    "intent"
  );
  assert_eq!(
    runtime.effects(),
    0,
    "approved input remains queued behind inspection"
  );
  assert_eq!(model.requests().await.len(), 2);
  host.release.notify_one();
  assert!(inspection.await.unwrap().is_ok());
  assert!(
    tokio::time::timeout(Duration::from_secs(5), model.wait_for_requests(3))
      .await
      .is_ok(),
    "no continuation: run={:?}, events={:?}",
    node.client.get_run(&run.id).await.unwrap(),
    node.daemon.store.events(&run.id, 0, 1000).unwrap()
  );
  model
    .push_reply(text_reply(protocol, "Queued input completed"))
    .await;
  assert_eq!(node.terminal(&run.id).await.status, "completed");
  assert_receipts(&node, &pending, "consumed", Some("completed"));
  assert_eq!(runtime.effects(), 1);
  drop(host);
  node.close().await;
}
