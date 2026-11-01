use crate::Daemon;
use crate::service_stream::EventStream;
use agent_protocol::v1::{
  self,
  client_service_server::{ClientService, ClientServiceServer},
};
use agent_security::Authenticate;
use tonic::{Request, Response, Status, service::interceptor::InterceptedService};
pub fn client_service(
  daemon: Daemon,
) -> InterceptedService<ClientServiceServer<Daemon>, Authenticate> {
  let auth = Authenticate::new(daemon.auth_token());
  ClientServiceServer::with_interceptor(daemon, auth)
}
pub(crate) fn db_error(error: anyhow::Error) -> Status {
  Status::failed_precondition(error.to_string())
}
#[tonic::async_trait]
impl ClientService for Daemon {
  async fn get_node(&self, _: Request<v1::Empty>) -> Result<Response<v1::Node>, Status> {
    let mut response = Response::new(v1::Node {
      id: self.node_id.clone(),
      name: "Lyonbrown".into(),
      os: std::env::consts::OS.into(),
      endpoint: format!("http://{}", self.config.listen),
      fingerprint: String::new(),
      capabilities: vec![
        "sessions".into(),
        "runs".into(),
        "runs.recover".into(),
        "workspace.read".into(),
        "workspace.write.approval".into(),
        "workspace.watch".into(),
        "decision.routing".into(),
        "tasks.manage".into(),
      ],
      protocol_version: 1,
      daemon_version: env!("CARGO_PKG_VERSION").into(),
      protocol_min: 1,
      protocol_max: 1,
      active_runs: self.active_count().await as u32,
      storage_schema_version: 1,
      draining: self.draining.load(std::sync::atomic::Ordering::Acquire),
    });
    // This authenticated, non-secret marker distinguishes live development incarnations.
    if let Ok(nonce) = std::env::var("LYONBROWN_DEV_READY_NONCE")
      && nonce.len() == 32
      && nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
      && let Ok(value) = nonce.parse()
    {
      response
        .metadata_mut()
        .insert("x-lyonbrown-dev-instance", value);
    }
    Ok(response)
  }
  async fn prepare_upgrade(&self, _: Request<v1::Empty>) -> Result<Response<v1::Node>, Status> {
    let _submission = self.submission.lock().await;
    self
      .draining
      .store(true, std::sync::atomic::Ordering::Release);
    self.get_node(Request::new(v1::Empty {})).await
  }
  async fn resume_accepting(&self, _: Request<v1::Empty>) -> Result<Response<v1::Node>, Status> {
    self
      .draining
      .store(false, std::sync::atomic::Ordering::Release);
    self.get_node(Request::new(v1::Empty {})).await
  }
  async fn list_sessions(&self, _: Request<v1::Empty>) -> Result<Response<v1::Sessions>, Status> {
    Ok(Response::new(v1::Sessions {
      sessions: self
        .store
        .sessions()
        .map_err(db_error)?
        .into_iter()
        .map(Into::into)
        .collect(),
    }))
  }
  async fn get_runtime_status(
    &self,
    request: Request<v1::SessionRequest>,
  ) -> Result<Response<v1::RuntimeStatus>, Status> {
    let session = self
      .store
      .session(&request.into_inner().session_id)
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    let runs = self.store.runs(&session.id).map_err(db_error)?;
    let active = runs.iter().find(|run| {
      matches!(
        run.status.as_str(),
        "accepted" | "running" | "awaiting_approval" | "cancel_requested"
      )
    });
    let pending_approvals = match active {
      Some(run) => self
        .store
        .approvals(&run.id)
        .map_err(db_error)?
        .into_iter()
        .filter(|approval| approval.status == "pending")
        .count() as u32,
      None => 0,
    };
    let node = self
      .get_node(Request::new(v1::Empty {}))
      .await?
      .into_inner();
    Ok(Response::new(v1::RuntimeStatus {
      node: Some(node),
      session: Some(session.into()),
      active_run: active.cloned().map(Into::into),
      provider: self.config.provider.clone(),
      model: self.config.model.clone(),
      pending_approvals,
      tools: 4 + u32::from(self.config.browser_tools) + u32::from(self.config.computer_tools),
      mcp_servers: self.config.mcp.len() as u32,
    }))
  }
  async fn list_providers(&self, _: Request<v1::Empty>) -> Result<Response<v1::Providers>, Status> {
    let uploaded = self
      .provider_secrets
      .list()
      .map_err(|_| Status::failed_precondition("provider credential store unavailable"))?;
    Ok(Response::new(v1::Providers {
      providers: self
        .config
        .provider_inventory
        .iter()
        .map(|status| {
          provider_status(
            status,
            uploaded
              .iter()
              .any(|item| item.id == secret_id(status.definition.id)),
          )
        })
        .collect(),
    }))
  }
  async fn put_provider_credential(
    &self,
    request: Request<v1::PutProviderCredentialRequest>,
  ) -> Result<Response<v1::Provider>, Status> {
    let mut input = request.into_inner();
    let definition = agent_config::provider_definition(&input.provider_id)
      .filter(|definition| {
        definition.runtime_supported && !definition.credential_env_names.is_empty()
      })
      .ok_or_else(|| Status::invalid_argument("provider does not accept uploaded credentials"))?;
    let value = String::from_utf8(std::mem::take(&mut input.credential))
      .map_err(|_| Status::invalid_argument("provider credential must be UTF-8"))?;
    let secret = agent_secrets::SecretString::from(value);
    self
      .provider_secrets
      .put(&secret_id(definition.id), &secret)
      .map_err(|_| Status::failed_precondition("provider credential could not be stored"))?;
    let status = self
      .config
      .provider_inventory
      .iter()
      .find(|status| status.definition.id == definition.id)
      .ok_or_else(|| Status::internal("provider inventory unavailable"))?;
    Ok(Response::new(provider_status(status, true)))
  }
  async fn remove_provider_credential(
    &self,
    request: Request<v1::RemoveProviderCredentialRequest>,
  ) -> Result<Response<v1::Provider>, Status> {
    let input = request.into_inner();
    let definition = agent_config::provider_definition(&input.provider_id)
      .ok_or_else(|| Status::invalid_argument("unknown provider"))?;
    self
      .provider_secrets
      .remove(&secret_id(definition.id))
      .map_err(|_| Status::failed_precondition("provider credential could not be removed"))?;
    let status = self
      .config
      .provider_inventory
      .iter()
      .find(|status| status.definition.id == definition.id)
      .ok_or_else(|| Status::internal("provider inventory unavailable"))?;
    Ok(Response::new(provider_status(status, false)))
  }
  async fn create_session(
    &self,
    request: Request<v1::CreateSessionRequest>,
  ) -> Result<Response<v1::Session>, Status> {
    let input = request.into_inner();
    let workspace = std::fs::canonicalize(&input.workspace)
      .map_err(|e| Status::invalid_argument(format!("workspace: {e}")))?;
    if !workspace.is_dir() {
      return Err(Status::invalid_argument("workspace must be a directory"));
    }
    let value = self
      .store
      .create_session(&input.title, &workspace.to_string_lossy(), &self.node_id)
      .map_err(db_error)?;
    Ok(Response::new(value.into()))
  }
  async fn get_session(
    &self,
    request: Request<v1::SessionRequest>,
  ) -> Result<Response<v1::Session>, Status> {
    let value = self
      .store
      .session(&request.into_inner().session_id)
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    Ok(Response::new(value.into()))
  }
  async fn start_run(
    &self,
    request: Request<v1::StartRunRequest>,
  ) -> Result<Response<v1::Run>, Status> {
    let _submission = self.submission.lock().await;
    if self.stopping.is_cancelled() || self.draining.load(std::sync::atomic::Ordering::Acquire) {
      return Err(Status::unavailable("daemon is shutting down"));
    }
    let input = request.into_inner();
    if self.active.lock().await.len() >= 128 {
      return Err(Status::resource_exhausted("node run capacity reached"));
    }
    let (value, created) = self
      .store
      .accept_run(
        &input.session_id,
        input.owner_epoch,
        &input.prompt,
        &input.idempotency_key,
      )
      .map_err(db_error)?;
    if created {
      self.launch(value.clone()).await.map_err(db_error)?;
    }
    self.events.notify_waiters();
    Ok(Response::new(value.into()))
  }
  async fn list_runs(
    &self,
    request: Request<v1::SessionRequest>,
  ) -> Result<Response<v1::Runs>, Status> {
    Ok(Response::new(v1::Runs {
      runs: self
        .store
        .runs(&request.into_inner().session_id)
        .map_err(db_error)?
        .into_iter()
        .map(Into::into)
        .collect(),
    }))
  }
  async fn get_run(&self, request: Request<v1::RunRequest>) -> Result<Response<v1::Run>, Status> {
    let value = self
      .store
      .run(&request.into_inner().run_id)
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("run not found"))?;
    Ok(Response::new(value.into()))
  }
  async fn get_run_recovery(
    &self,
    request: Request<v1::RunRequest>,
  ) -> Result<Response<v1::RunRecovery>, Status> {
    self
      .inspect_run_recovery(&request.into_inner().run_id)
      .await
      .map(Response::new)
  }
  async fn recover_run(
    &self,
    request: Request<v1::RecoverRunRequest>,
  ) -> Result<Response<v1::RunRecoveryResult>, Status> {
    self
      .resolve_run_recovery(request.into_inner())
      .await
      .map(Response::new)
  }
  type WatchRunStream = EventStream;
  async fn watch_run(
    &self,
    request: Request<v1::WatchRequest>,
  ) -> Result<Response<Self::WatchRunStream>, Status> {
    self.event_stream(request.into_inner()).map(Response::new)
  }
  async fn cancel_run(
    &self,
    request: Request<v1::RunRequest>,
  ) -> Result<Response<v1::Run>, Status> {
    let id = request.into_inner().run_id;
    let run = self
      .store
      .run(&id)
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("run not found"))?;
    let session = self
      .store
      .session(&run.session_id)
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if matches!(
      run.status.as_str(),
      "completed" | "failed" | "cancelled" | "uncertain"
    ) {
      return Ok(Response::new(run.into()));
    }
    let value = self
      .store
      .update_run_owned(
        &id,
        session.owner_epoch,
        agent_core::RunStatus::CancelRequested,
        "",
        "",
      )
      .map_err(db_error)?;
    if let Some(token) = self.active.lock().await.get(&id) {
      token.cancel();
    }
    self.events.notify_waiters();
    Ok(Response::new(value.into()))
  }
  async fn list_approvals(
    &self,
    request: Request<v1::RunRequest>,
  ) -> Result<Response<v1::Approvals>, Status> {
    Ok(Response::new(v1::Approvals {
      approvals: self
        .store
        .approvals(&request.into_inner().run_id)
        .map_err(db_error)?
        .into_iter()
        .map(Into::into)
        .collect(),
    }))
  }
  async fn approve_action(
    &self,
    request: Request<v1::ApproveRequest>,
  ) -> Result<Response<v1::Approval>, Status> {
    let input = request.into_inner();
    let value = self
      .store
      .decide_approval(&input.approval_id, input.approved)
      .map_err(db_error)?;
    self.approvals.notify_waiters();
    self.events.notify_waiters();
    Ok(Response::new(value.into()))
  }
}

fn provider_status(status: &agent_config::ProviderStatus, uploaded: bool) -> v1::Provider {
  use agent_config::{CredentialSource, CredentialState, ProviderProtocol};
  v1::Provider {
    id: status.definition.id.into(),
    name: status.definition.name.into(),
    protocols: status
      .definition
      .protocols
      .iter()
      .map(|protocol| {
        match protocol {
          ProviderProtocol::OpenaiResponses => "openai_responses",
          ProviderProtocol::OpenaiCompletions => "openai_completions",
          ProviderProtocol::AnthropicMessages => "anthropic_messages",
          ProviderProtocol::Local => "local",
        }
        .into()
      })
      .collect(),
    runtime_supported: status.definition.runtime_supported,
    selected: status.selected,
    credential_state: if uploaded {
      "detected"
    } else {
      match status.credential.state {
        CredentialState::Missing => "missing",
        CredentialState::Detected => "detected",
        CredentialState::Invalid => "invalid",
        CredentialState::NotRequired => "not_required",
      }
    }
    .into(),
    credential_source: if uploaded {
      "uploaded_encrypted"
    } else {
      status
        .credential
        .source
        .map(|source| match source {
          CredentialSource::DaemonEnvironment => "daemon_environment",
          CredentialSource::ClientEnvironment => "client_environment",
          CredentialSource::Dotenv => "dotenv",
        })
        .unwrap_or_default()
    }
    .into(),
    credential_environment: status
      .credential
      .environment_name
      .clone()
      .unwrap_or_default(),
    models: status
      .configured_models
      .iter()
      .map(|model| v1::ProviderModel {
        id: model.id.clone(),
        selected: model.selected,
      })
      .collect(),
  }
}

fn secret_id(provider: &str) -> String {
  format!("provider-{provider}")
}
