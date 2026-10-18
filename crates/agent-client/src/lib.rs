//! Authenticated daemon client shared by terminal and native desktop clients.
mod browser;
mod computer;
mod development;
mod orchestration;
mod workspace;
use agent_protocol::v1::{self, client_service_client::ClientServiceClient};
use anyhow::{Context, Result, ensure};
use tonic::service::interceptor::InterceptedService;
use tonic::transport::{Channel, Endpoint};
use tonic::{Request, Status, metadata::MetadataValue, service::Interceptor};

#[derive(Clone)]
struct Authentication(MetadataValue<tonic::metadata::Ascii>);
impl Interceptor for Authentication {
  fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
    request
      .metadata_mut()
      .insert("authorization", self.0.clone());
    Ok(request)
  }
}

#[derive(Clone)]
pub struct AgentClient {
  inner: ClientServiceClient<InterceptedService<Channel, Authentication>>,
  workspace: v1::workspace_service_client::WorkspaceServiceClient<
    InterceptedService<Channel, Authentication>,
  >,
  browser:
    v1::browser_service_client::BrowserServiceClient<InterceptedService<Channel, Authentication>>,
  browser_runtime: v1::browser_runtime_service_client::BrowserRuntimeServiceClient<
    InterceptedService<Channel, Authentication>,
  >,
  computer_runtime: v1::computer_runtime_service_client::ComputerRuntimeServiceClient<
    InterceptedService<Channel, Authentication>,
  >,
  orchestration: v1::orchestration_service_client::OrchestrationServiceClient<
    InterceptedService<Channel, Authentication>,
  >,
}
impl AgentClient {
  pub async fn connect(endpoint: &str, token: &str) -> Result<Self> {
    let client = Self::connect_authenticated(endpoint, token).await?;
    let node = client.node().await.context("daemon handshake")?;
    ensure!(
      node.protocol_min <= 1 && node.protocol_max >= 1,
      "incompatible daemon protocol range {}..={} (client requires 1)",
      node.protocol_min,
      node.protocol_max
    );
    Ok(client)
  }
  /// Maintenance probe only: authenticates credentials without assuming protocol compatibility.
  pub async fn connect_authenticated(endpoint: &str, token: &str) -> Result<Self> {
    agent_security::initialize_tls();
    ensure!(!token.trim().is_empty(), "daemon token is empty");
    let url = url::Url::parse(endpoint).map_err(|_| anyhow::anyhow!("invalid daemon endpoint"))?;
    ensure!(
      matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none(),
      "invalid daemon endpoint"
    );
    let loopback = match url.host() {
      Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
      Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
      Some(url::Host::Domain("localhost")) => true,
      _ => false,
    };
    ensure!(
      url.scheme() == "https" || loopback,
      "remote daemon connections require HTTPS"
    );
    let mut endpoint = Endpoint::from_shared(endpoint.to_owned())
      .context("invalid daemon endpoint")?
      .connect_timeout(std::time::Duration::from_secs(5));
    if url.scheme() == "https" {
      endpoint =
        endpoint.tls_config(tonic::transport::ClientTlsConfig::new().with_webpki_roots())?;
    }
    let channel = endpoint.connect().await.context("connect to agentd")?;
    let mut bearer: MetadataValue<tonic::metadata::Ascii> = format!("Bearer {}", token.trim())
      .parse()
      .context("invalid daemon token")?;
    bearer.set_sensitive(true);
    let auth = Authentication(bearer);
    let client = Self {
      orchestration: v1::orchestration_service_client::OrchestrationServiceClient::with_interceptor(
        channel.clone(),
        auth.clone(),
      ),
      inner: ClientServiceClient::with_interceptor(channel.clone(), auth.clone()),
      workspace: v1::workspace_service_client::WorkspaceServiceClient::with_interceptor(
        channel.clone(),
        auth.clone(),
      ),
      browser: v1::browser_service_client::BrowserServiceClient::with_interceptor(
        channel.clone(),
        auth.clone(),
      )
      .max_decoding_message_size(18 * 1024 * 1024),
      browser_runtime:
        v1::browser_runtime_service_client::BrowserRuntimeServiceClient::with_interceptor(
          channel.clone(),
          auth.clone(),
        )
        .max_decoding_message_size(18 * 1024 * 1024)
        .max_encoding_message_size(18 * 1024 * 1024),
      computer_runtime:
        v1::computer_runtime_service_client::ComputerRuntimeServiceClient::with_interceptor(
          channel, auth,
        )
        .max_decoding_message_size(agent_protocol::COMPUTER_RUNTIME_MESSAGE_LIMIT)
        .max_encoding_message_size(agent_protocol::COMPUTER_RUNTIME_MESSAGE_LIMIT),
    };
    client
      .node()
      .await
      .context("authenticated daemon handshake")?;
    Ok(client)
  }
  pub async fn prepare_upgrade(&self) -> Result<v1::Node> {
    Ok(
      self
        .inner
        .clone()
        .prepare_upgrade(v1::Empty {})
        .await?
        .into_inner(),
    )
  }
  pub async fn resume_accepting(&self) -> Result<v1::Node> {
    Ok(
      self
        .inner
        .clone()
        .resume_accepting(v1::Empty {})
        .await?
        .into_inner(),
    )
  }
  pub async fn node(&self) -> Result<v1::Node> {
    Ok(
      self
        .inner
        .clone()
        .get_node(v1::Empty {})
        .await?
        .into_inner(),
    )
  }
  pub async fn list_sessions(&self) -> Result<Vec<v1::Session>> {
    Ok(
      self
        .inner
        .clone()
        .list_sessions(v1::Empty {})
        .await?
        .into_inner()
        .sessions,
    )
  }
  pub async fn create_session(&self, title: &str, workspace: &str) -> Result<v1::Session> {
    Ok(
      self
        .inner
        .clone()
        .create_session(v1::CreateSessionRequest {
          title: title.into(),
          workspace: workspace.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn start_run(
    &self,
    session_id: &str,
    prompt: &str,
    idempotency_key: &str,
  ) -> Result<v1::Run> {
    Ok(
      self
        .inner
        .clone()
        .start_run(v1::StartRunRequest {
          session_id: session_id.into(),
          prompt: prompt.into(),
          idempotency_key: idempotency_key.into(),
          owner_epoch: 1,
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn get_run(&self, run_id: &str) -> Result<v1::Run> {
    Ok(
      self
        .inner
        .clone()
        .get_run(v1::RunRequest {
          run_id: run_id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn list_runs(&self, session_id: &str) -> Result<Vec<v1::Run>> {
    Ok(
      self
        .inner
        .clone()
        .list_runs(v1::SessionRequest {
          session_id: session_id.into(),
        })
        .await?
        .into_inner()
        .runs,
    )
  }
  pub async fn watch_run(
    &self,
    run_id: &str,
    after_cursor: u64,
  ) -> Result<tonic::Streaming<v1::Event>> {
    Ok(
      self
        .inner
        .clone()
        .watch_run(v1::WatchRequest {
          run_id: run_id.into(),
          after_cursor,
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn cancel_run(&self, run_id: &str) -> Result<v1::Run> {
    Ok(
      self
        .inner
        .clone()
        .cancel_run(v1::RunRequest {
          run_id: run_id.into(),
        })
        .await?
        .into_inner(),
    )
  }
  pub async fn list_approvals(&self, run_id: &str) -> Result<Vec<v1::Approval>> {
    Ok(
      self
        .inner
        .clone()
        .list_approvals(v1::RunRequest {
          run_id: run_id.into(),
        })
        .await?
        .into_inner()
        .approvals,
    )
  }
  pub async fn approve(&self, approval_id: &str, approved: bool) -> Result<v1::Approval> {
    Ok(
      self
        .inner
        .clone()
        .approve_action(v1::ApproveRequest {
          approval_id: approval_id.into(),
          approved,
        })
        .await?
        .into_inner(),
    )
  }
}
