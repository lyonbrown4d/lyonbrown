use agent_client::AgentClient;
use agent_test_support::{MockModelServer, StandardProtocol};
use agentd::{Daemon, config::Config};
use clap::Parser;
use tempfile::TempDir;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;

pub struct TestNode {
  pub daemon: Daemon,
  pub client: AgentClient,
  pub workspace: TempDir,
  pub session_id: String,
  _data: TempDir,
  server: JoinHandle<()>,
}
impl TestNode {
  pub async fn start(
    main: &MockModelServer,
    classifier: &MockModelServer,
    protocol: StandardProtocol,
  ) -> Self {
    Self::start_configured(main, classifier, protocol, |_| {}).await
  }
  pub async fn start_configured(
    main: &MockModelServer,
    classifier: &MockModelServer,
    protocol: StandardProtocol,
    configure: impl FnOnce(&mut Config),
  ) -> Self {
    let data = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = Config::try_parse_from(["agentd"]).unwrap();
    config.listen = address;
    config.data_dir = Some(data.path().into());
    config.provider = protocol.provider().into();
    config.model = "main-fixture".into();
    config.api_base = Some(main.base_url().into());
    config.provider_api_key = Some("synthetic-main-secret".into());
    config.decision.model = Some("decision-fixture".into());
    config.decision.api_base = Some(classifier.base_url().into());
    config.decision_api_key = Some("synthetic-decision-secret".into());
    config.run_timeout_secs = 10;
    configure(&mut config);
    let daemon = Daemon::open(config).await.unwrap();
    daemon.recover().await.unwrap();
    let service = agentd::client_service(daemon.clone());
    let workspace_service = agentd::workspace_service(daemon.clone());
    let browser_service = agentd::browser_service(daemon.clone());
    let runtime_service = agentd::runtime_service(daemon.clone());
    let server = tokio::spawn(async move {
      tonic::transport::Server::builder()
        .add_service(service)
        .add_service(workspace_service)
        .add_service(browser_service)
        .add_service(runtime_service)
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
        .unwrap();
    });
    let client = AgentClient::connect(&format!("http://{address}"), daemon.auth_token())
      .await
      .unwrap();
    let session = client
      .create_session("decision acceptance", workspace.path().to_str().unwrap())
      .await
      .unwrap();
    Self {
      daemon,
      client,
      workspace,
      session_id: session.id,
      _data: data,
      server,
    }
  }
  pub async fn run(&self, prompt: &str) -> agent_protocol::v1::Run {
    self
      .client
      .start_run(&self.session_id, prompt, &agent_core::new_id())
      .await
      .unwrap()
  }
  pub async fn terminal(&self, id: &str) -> agent_protocol::v1::Run {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
      let mut stream = self.client.watch_run(id, 0).await.unwrap();
      while stream.message().await.unwrap().is_some() {}
      self.client.get_run(id).await.unwrap()
    })
    .await
    .unwrap()
  }
  pub async fn close(self) {
    self.daemon.shutdown().await;
    self.server.abort();
  }
}
