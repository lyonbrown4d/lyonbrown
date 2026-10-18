mod attachment;
mod response;

use crate::support::TestNode;
use agent_computer::*;
use agent_computer_runtime::{
  ComputerRuntime, RUNTIME_PROTOCOL_VERSION, RunBinding, RuntimeHello, wire,
};
use agent_protocol::v1;
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

#[derive(Default)]
pub struct Calls {
  pub capabilities: AtomicUsize,
  pub effects: AtomicUsize,
}
struct Backend {
  identity: DesktopIdentity,
  calls: Arc<Calls>,
}
impl DesktopBackend for Backend {
  fn capabilities(&mut self) -> Result<DesktopCapabilities, DesktopError> {
    self.calls.capabilities.fetch_add(1, Ordering::SeqCst);
    Ok(capabilities(self.identity.clone()))
  }
  fn observe(
    &mut self,
    _: &DesktopSession,
    _: ObservationLimits,
    _: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError> {
    self.calls.effects.fetch_add(1, Ordering::SeqCst);
    panic!("capability inspection must not capture")
  }
  fn validate_snapshot(&mut self, _: &DesktopSnapshot) -> Result<(), DesktopError> {
    self.calls.effects.fetch_add(1, Ordering::SeqCst);
    panic!("capability inspection must not validate snapshots")
  }
  fn execute(
    &mut self,
    _: &DesktopAction,
    _: &DesktopSnapshot,
    _: &ExecutionGuard,
  ) -> ActionOutcome {
    self.calls.effects.fetch_add(1, Ordering::SeqCst);
    panic!("capability inspection must not execute input")
  }
}
pub fn capabilities(identity: DesktopIdentity) -> DesktopCapabilities {
  DesktopCapabilities {
    identity,
    interactive: true,
    capture: PermissionState::Granted,
    input: PermissionState::Granted,
    accessibility: PermissionState::Unsupported,
  }
}
pub fn desktop(node: &TestNode) -> DesktopSession {
  DesktopSession {
    id: Uuid::new_v4(),
    identity: DesktopIdentity {
      node_id: node.daemon.node_id.clone(),
      user_session: "synthetic-interactive-user".into(),
    },
    scope: TargetScope::Window(7),
  }
}
pub fn runtime(session: DesktopSession) -> (Arc<ComputerRuntime>, Arc<Calls>) {
  let calls = Arc::new(Calls::default());
  let backend = Backend {
    identity: session.identity.clone(),
    calls: calls.clone(),
  };
  (
    Arc::new(ComputerRuntime::new(
      session,
      Box::new(backend),
      Arc::new(LeaseRegistry::default()),
    )),
    calls,
  )
}
pub fn binding(node: &TestNode, session: &str) -> RunBinding {
  let (run, _) = node
    .daemon
    .store
    .accept_run(session, 1, "inspect capabilities", &agent_core::new_id())
    .unwrap();
  node
    .daemon
    .store
    .update_run_owned(&run.id, 1, agent_core::RunStatus::Running, "", "")
    .unwrap();
  RunBinding {
    daemon_session_id: session.into(),
    run_id: run.id,
    owner_epoch: 1,
  }
}
pub fn hello(host: &str, session: &str, desktop: &DesktopSession) -> v1::ComputerRuntimeResponse {
  v1::ComputerRuntimeResponse {
    host_id: host.into(),
    daemon_session_id: session.into(),
    protocol_version: RUNTIME_PROTOCOL_VERSION,
    request_id: String::new(),
    payload: wire::encode(&RuntimeHello {
      desktop_session: desktop.clone(),
    })
    .unwrap(),
  }
}
pub struct RawHost {
  pub host: String,
  pub sender: mpsc::Sender<v1::ComputerRuntimeResponse>,
  pub requests: tonic::Streaming<v1::ComputerRuntimeRequest>,
}
impl RawHost {
  pub async fn next(&mut self) -> v1::ComputerRuntimeRequest {
    tokio::time::timeout(std::time::Duration::from_secs(3), self.requests.message())
      .await
      .unwrap()
      .unwrap()
      .unwrap()
  }
  pub async fn attach(node: &TestNode, desktop: &DesktopSession) -> Self {
    let host = Uuid::new_v4().to_string();
    let (sender, receiver) = mpsc::channel(2);
    sender
      .send(hello(&host, &node.session_id, desktop))
      .await
      .unwrap();
    let requests = node
      .client
      .attach_computer_runtime(ReceiverStream::new(receiver))
      .await
      .unwrap();
    Self {
      host,
      sender,
      requests,
    }
  }
}
pub async fn detached(node: &TestNode) {
  tokio::time::timeout(std::time::Duration::from_secs(3), async {
    loop {
      if node
        .daemon
        .computer_runtime_info(&node.session_id)
        .await
        .unwrap()
        .is_none()
      {
        break;
      }
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
}

pub async fn reject_unauthenticated(node: &TestNode) {
  use tokio_stream::wrappers::TcpListenerStream;
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let service = agentd::computer_runtime_service(node.daemon.clone());
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(service)
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
      .unwrap();
  });
  let mut client = v1::computer_runtime_service_client::ComputerRuntimeServiceClient::connect(
    format!("http://{address}"),
  )
  .await
  .unwrap();
  let (sender, receiver) = mpsc::channel(1);
  sender
    .send(hello(
      &Uuid::new_v4().to_string(),
      &node.session_id,
      &desktop(node),
    ))
    .await
    .unwrap();
  let mut request = tonic::Request::new(ReceiverStream::new(receiver));
  request.metadata_mut().insert(
    "authorization",
    "Bearer synthetic-invalid-token".parse().unwrap(),
  );
  let error = match client.attach(request).await {
    Err(error) => error,
    Ok(_) => panic!("unauthenticated host must fail"),
  };
  assert_eq!(error.code(), tonic::Code::Unauthenticated);
  assert!(
    node
      .daemon
      .computer_runtime_info(&node.session_id)
      .await
      .unwrap()
      .is_none()
  );
  server.abort();
}
