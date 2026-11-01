use std::time::Duration;

use agent_client::AgentClient;
use agent_protocol::v1;
use agent_test_support::{MockModelServer, StandardProtocol};
use tokio_stream::wrappers::TcpListenerStream;

use crate::peer_support::Pair;

pub struct Remote {
  pub pair: Pair,
  pub client: AgentClient,
  pub endpoint: String,
  server: tokio::task::JoinHandle<()>,
}

impl Remote {
  pub async fn open(mock: &MockModelServer, protocol: StandardProtocol) -> Self {
    Self::from_pair(Pair::open(mock, protocol).await).await
  }

  pub async fn from_pair(mut pair: Pair) -> Self {
    pair.serve().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let daemon = pair.master.clone();
    let server = tokio::spawn(async move {
      tonic::transport::Server::builder()
        .add_service(agentd::client_service(daemon.clone()))
        .add_service(agentd::orchestration_service(daemon.clone()))
        .add_service(agentd::workspace_service(daemon.clone()))
        .add_service(agentd::browser_service(daemon))
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
        .unwrap();
    });
    let client = AgentClient::connect(&endpoint, pair.master.auth_token())
      .await
      .unwrap();
    Self {
      pair,
      client,
      endpoint,
      server,
    }
  }

  pub fn envelope(&self) -> v1::TaskEnvelope {
    self.pair.envelope(vec!["workspace.read".into()]).into()
  }

  pub fn target(&self) -> v1::Node {
    self.pair.worker_manager.local_node()
  }

  pub async fn terminal(&self, id: &str) -> v1::Task {
    tokio::time::timeout(Duration::from_secs(10), async {
      loop {
        let task = self.client.reconcile_task(self.target(), id).await.unwrap();
        if matches!(
          task.attempt_status.as_str(),
          "succeeded" | "failed" | "cancelled" | "uncertain"
        ) {
          return task;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .unwrap()
  }

  pub async fn close(self) {
    self.server.abort();
    self.pair.stop().await;
  }
}

pub fn code(error: anyhow::Error) -> tonic::Code {
  error.downcast_ref::<tonic::Status>().unwrap().code()
}
