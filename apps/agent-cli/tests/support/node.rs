use agent_client::AgentClient;
use agent_protocol::v1::Run;
use agent_test_support::{MockModelServer, StandardProtocol};
use agentd::{Daemon, config::Config};
use clap::Parser;
use std::{net::SocketAddr, time::Duration};
use tempfile::TempDir;
use tokio::{
  net::{TcpListener, TcpStream},
  task::{JoinHandle, JoinSet},
};
use tokio_stream::wrappers::TcpListenerStream;

pub struct Node {
  pub daemon: Daemon,
  pub client: AgentClient,
  pub workspace: TempDir,
  pub data: TempDir,
  address: SocketAddr,
  daemon_address: SocketAddr,
  server: JoinHandle<()>,
  transport: JoinHandle<()>,
}
impl Node {
  pub async fn start(model: &MockModelServer) -> Self {
    let data = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let daemon_address = listener.local_addr().unwrap();
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = proxy.local_addr().unwrap();
    let mut config = Config::try_parse_from(["agentd"]).unwrap();
    config.listen = daemon_address;
    config.data_dir = Some(data.path().into());
    config.provider = StandardProtocol::ChatCompletions.provider().into();
    config.model = "terminal-fixture".into();
    config.api_base = Some(model.base_url().into());
    config.provider_api_key = Some("synthetic-test-secret".into());
    config.run_timeout_secs = 60;
    let daemon = Daemon::open(config).await.unwrap();
    daemon.recover().await.unwrap();
    let server = Self::serve(daemon.clone(), listener);
    let transport = Self::forward(proxy, daemon_address);
    let client = AgentClient::connect(&format!("http://{daemon_address}"), daemon.auth_token())
      .await
      .unwrap();
    Self {
      daemon,
      client,
      workspace,
      data,
      address,
      daemon_address,
      server,
      transport,
    }
  }
  // Opaque TCP forwarding allows a real network disconnect without touching daemon jobs.
  fn forward(listener: TcpListener, target: SocketAddr) -> JoinHandle<()> {
    tokio::spawn(async move {
      let mut connections = JoinSet::new();
      loop {
        tokio::select! {
          accepted = listener.accept() => {
            let (mut downstream, _) = accepted.unwrap();
            connections.spawn(async move {
              if let Ok(mut upstream) = TcpStream::connect(target).await {
                let _ = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await;
              }
            });
          }
          _ = connections.join_next(), if !connections.is_empty() => {}
        }
      }
    })
  }
  fn serve(daemon: Daemon, listener: TcpListener) -> JoinHandle<()> {
    tokio::spawn(async move {
      tonic::transport::Server::builder()
        .add_service(agentd::client_service(daemon.clone()))
        .add_service(agentd::orchestration_service(daemon))
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
        .unwrap();
    })
  }
  pub fn endpoint(&self) -> String {
    format!("http://{}", self.address)
  }
  pub async fn disconnect(&self) {
    self.transport.abort();
    while !self.transport.is_finished() {
      tokio::task::yield_now().await;
    }
  }
  pub async fn reconnect(&mut self) {
    let listener = TcpListener::bind(self.address).await.unwrap();
    self.transport = Self::forward(listener, self.daemon_address);
  }
  pub async fn run_count(&self, session: &str, count: usize) -> Vec<Run> {
    tokio::time::timeout(Duration::from_secs(12), async {
      loop {
        let runs = self.client.list_runs(session).await.unwrap();
        if runs.len() == count {
          return runs;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
    })
    .await
    .expect("durable run count")
  }
  pub async fn status(&self, id: &str, status: &str) -> Run {
    tokio::time::timeout(Duration::from_secs(12), async {
      loop {
        let run = self.client.get_run(id).await.unwrap();
        if run.status == status {
          return run;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
      }
    })
    .await
    .unwrap_or_else(|_| panic!("run {id} never reached {status}"))
  }
  pub async fn close(self) {
    self.daemon.shutdown().await;
    self.server.abort();
    self.transport.abort();
  }
}
impl Drop for Node {
  fn drop(&mut self) {
    self.server.abort();
    self.transport.abort();
  }
}
