//! Typed authenticated UI access to daemon-owned plans and checkpoints.
mod changes;
mod convert;
mod plans;
use crate::Daemon;
use agent_core::{Run, Session};
use agent_protocol::v1;
use agent_protocol::v1::workflow_service_server::{WorkflowService, WorkflowServiceServer};
use agent_security::Authenticate;
use tonic::{Status, service::interceptor::InterceptedService};
pub fn workflow_service(
  daemon: Daemon,
) -> InterceptedService<WorkflowServiceServer<Daemon>, Authenticate> {
  let auth = Authenticate::new(daemon.auth_token());
  InterceptedService::new(
    WorkflowServiceServer::new(daemon)
      .max_decoding_message_size(12 * 1024 * 1024)
      .max_encoding_message_size(12 * 1024 * 1024),
    auth,
  )
}
async fn session(daemon: &Daemon, id: &str) -> Result<Session, Status> {
  let session = daemon
    .store
    .session(id)
    .await
    .map_err(error)?
    .ok_or_else(|| Status::not_found("session not found"))?;
  if session.owner_node_id != daemon.node_id {
    return Err(Status::failed_precondition(
      "session is owned by another node",
    ));
  }
  Ok(session)
}
async fn binding(
  daemon: &Daemon,
  session_id: &str,
  run_id: &str,
) -> Result<(Session, Run), Status> {
  let session = session(daemon, session_id).await?;
  let run = daemon
    .store
    .run(run_id)
    .await
    .map_err(error)?
    .filter(|run| run.session_id == session_id)
    .ok_or_else(|| Status::not_found("run not found in session"))?;
  Ok((session, run))
}
fn error(error: anyhow::Error) -> Status {
  use agent_planning::PlanError;
  match error.downcast_ref::<PlanError>() {
    Some(PlanError::Conflict | PlanError::RequestConflict) => {
      Status::aborted("plan revision or request conflict")
    }
    Some(PlanError::NotFound) => Status::not_found("plan or step not found"),
    Some(PlanError::Forbidden) => Status::permission_denied("plan authority required"),
    Some(PlanError::Invalid(_)) => Status::invalid_argument(error.to_string()),
    Some(PlanError::VerificationRequired) => Status::failed_precondition(error.to_string()),
    None => Status::failed_precondition(agent_storage::public_error_message(&error)),
  }
}
#[tonic::async_trait]
impl WorkflowService for Daemon {
  async fn list_plans(
    &self,
    request: tonic::Request<v1::ListWorkflowPlansRequest>,
  ) -> Result<tonic::Response<v1::WorkflowPlans>, Status> {
    plans::list(self, request.into_inner())
      .await
      .map(tonic::Response::new)
  }
  async fn get_plan(
    &self,
    request: tonic::Request<v1::GetWorkflowPlanRequest>,
  ) -> Result<tonic::Response<v1::WorkflowPlan>, Status> {
    plans::get(self, request.into_inner())
      .await
      .map(tonic::Response::new)
  }
  async fn mutate_plan(
    &self,
    request: tonic::Request<v1::MutateWorkflowPlanRequest>,
  ) -> Result<tonic::Response<v1::WorkflowPlan>, Status> {
    plans::mutate(self, request.into_inner())
      .await
      .map(tonic::Response::new)
  }
  async fn list_changes(
    &self,
    request: tonic::Request<v1::ListWorkflowChangesRequest>,
  ) -> Result<tonic::Response<v1::WorkflowChanges>, Status> {
    changes::list(self, request.into_inner())
      .await
      .map(tonic::Response::new)
  }
  async fn get_change(
    &self,
    request: tonic::Request<v1::GetWorkflowChangeRequest>,
  ) -> Result<tonic::Response<v1::WorkflowChange>, Status> {
    changes::get(self, request.into_inner())
      .await
      .map(tonic::Response::new)
  }
  async fn preview_undo(
    &self,
    request: tonic::Request<v1::PreviewWorkflowUndoRequest>,
  ) -> Result<tonic::Response<v1::WorkflowChange>, Status> {
    changes::undo(self, request.into_inner())
      .await
      .map(tonic::Response::new)
  }
  async fn apply_change(
    &self,
    request: tonic::Request<v1::ApplyWorkflowChangeRequest>,
  ) -> Result<tonic::Response<v1::WorkflowChange>, Status> {
    changes::apply(self, request.into_inner())
      .await
      .map(tonic::Response::new)
  }
}
