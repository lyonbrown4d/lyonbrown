#[cfg(unix)]
pub mod config_process;
mod isolation;
mod lifecycle;
mod server;
use agent_client::AgentClient;
use agent_mcp::McpServerConfig;
use agent_test_support::{MockModelServer, StandardProtocol};
use agentd::{Daemon, config::Config};
use clap::Parser;
pub use server::Fixture;
use std::time::Duration;
use tempfile::TempDir;
use tokio_stream::wrappers::TcpListenerStream;

pub struct Node {
  pub daemon: Daemon,
  pub client: AgentClient,
  pub session: String,
  _data: TempDir,
  _workspace: TempDir,
  service: tokio::task::JoinHandle<()>,
}
impl Node {
  pub async fn start(
    main: &MockModelServer,
    protocol: StandardProtocol,
    mcp: Option<McpServerConfig>,
  ) -> Self {
    let data = tempfile::tempdir().expect("node data");
    let workspace = tempfile::tempdir().expect("workspace");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
      .await
      .expect("client loopback");
    let address = listener.local_addr().expect("client address");
    let mut config = Config::try_parse_from(["agentd"]).expect("CLI config");
    config.listen = address;
    config.data_dir = Some(data.path().to_owned());
    config.provider = protocol.provider().into();
    config.model = "main-fixture".into();
    config.api_base = Some(main.base_url().into());
    config.provider_api_key = Some("synthetic-model-secret".into());
    config.run_timeout_secs = 10;
    config.mcp = mcp.into_iter().collect();
    let daemon = Daemon::open(config).await.expect("open daemon");
    daemon.recover().await.expect("recover daemon");
    let endpoint = agentd::client_service(daemon.clone());
    let service = tokio::spawn(async move {
      tonic::transport::Server::builder()
        .add_service(endpoint)
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
        .expect("client service");
    });
    let client = AgentClient::connect(&format!("http://{address}"), daemon.auth_token())
      .await
      .expect("authenticated client");
    let session = client
      .create_session(
        "MCP acceptance",
        workspace.path().to_str().expect("workspace UTF-8"),
      )
      .await
      .expect("session")
      .id;
    Self {
      daemon,
      client,
      session,
      _data: data,
      _workspace: workspace,
      service,
    }
  }
  pub async fn run(&self, message: &str) -> agent_protocol::v1::Run {
    self
      .client
      .start_run(&self.session, message, &agent_core::new_id())
      .await
      .expect("start run")
  }
  pub async fn pending(&self, id: &str) -> agent_protocol::v1::Approval {
    tokio::time::timeout(Duration::from_secs(5), async {
      let mut events = self.client.watch_run(id, 0).await.expect("watch approvals");
      while events.message().await.expect("event").is_some() {
        if let Some(approval) = self
          .client
          .list_approvals(id)
          .await
          .expect("approvals")
          .into_iter()
          .next()
        {
          return approval;
        }
      }
      panic!("run ended before MCP approval");
    })
    .await
    .expect("approval deadline")
  }
  pub async fn terminal(&self, id: &str) -> agent_protocol::v1::Run {
    tokio::time::timeout(Duration::from_secs(8), async {
      let mut events = self.client.watch_run(id, 0).await.expect("watch terminal");
      while events.message().await.expect("event").is_some() {}
      self.client.get_run(id).await.expect("terminal run")
    })
    .await
    .expect("terminal deadline")
  }
  pub async fn close(self) {
    self.daemon.shutdown().await;
    self.service.abort();
  }
}
