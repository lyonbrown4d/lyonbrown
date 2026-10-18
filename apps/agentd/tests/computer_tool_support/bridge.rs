//! Authenticated test bridge: run the real engine, then fault its Execute reply.
use super::Runtime;
use crate::support::TestNode;
use agent_computer_runtime::{Command, Operation, RUNTIME_PROTOCOL_VERSION, RuntimeHello, wire};
use agent_protocol::v1;
use std::sync::Arc;
use tokio::sync::{Notify, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

pub struct LostReceiptHost {
  pub host_id: String,
  lost: Arc<Notify>,
  task: tokio::task::JoinHandle<()>,
}
impl LostReceiptHost {
  pub async fn attach(node: &TestNode, runtime: &Runtime) -> Self {
    Self::attach_fault(node, runtime, false).await
  }
  pub async fn attach_corrupted(node: &TestNode, runtime: &Runtime) -> Self {
    Self::attach_fault(node, runtime, true).await
  }
  async fn attach_fault(node: &TestNode, runtime: &Runtime, corrupt: bool) -> Self {
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
    let engine = runtime.engine.clone();
    let lost = Arc::new(Notify::new());
    let notify = lost.clone();
    let host = host_id.clone();
    let task = tokio::spawn(async move {
      while let Some(request) = requests.message().await.unwrap() {
        let command: Command = wire::decode(&request.payload).unwrap();
        let execute = matches!(command.operation, Operation::Execute { .. });
        let engine = engine.clone();
        let mut result =
          tokio::task::spawn_blocking(move || engine.handle(command, CancellationToken::new()))
            .await
            .unwrap();
        if execute {
          assert!(
            matches!(result, Ok(agent_computer_runtime::Reply::Executed(_))),
            "effect must finish in real engine before losing receipt"
          );
          notify.notify_one();
          if corrupt {
            if let Ok(agent_computer_runtime::Reply::Executed(executed)) = &mut result {
              executed.original_request.action =
                agent_computer::DesktopAction::Move { x: 11, y: 10 };
            }
          } else {
            std::future::pending::<()>().await;
          }
        }
        sender
          .send(v1::ComputerRuntimeResponse {
            request_id: request.request_id,
            host_id: host.clone(),
            daemon_session_id: session.clone(),
            protocol_version: RUNTIME_PROTOCOL_VERSION,
            payload: wire::encode(&result).unwrap(),
          })
          .await
          .unwrap();
      }
    });
    Self {
      host_id,
      lost,
      task,
    }
  }
  pub async fn wait_lost(&self) {
    tokio::time::timeout(std::time::Duration::from_secs(5), self.lost.notified())
      .await
      .unwrap();
  }
}
impl Drop for LostReceiptHost {
  fn drop(&mut self) {
    self.task.abort();
  }
}
