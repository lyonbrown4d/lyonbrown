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
    Ok(Response::new(v1::Node {
      id: self.node_id.clone(),
      name: "Lyonbrown".into(),
      os: std::env::consts::OS.into(),
      endpoint: format!("http://{}", self.config.listen),
      fingerprint: String::new(),
      capabilities: vec![
        "sessions".into(),
        "runs".into(),
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
    }))
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
