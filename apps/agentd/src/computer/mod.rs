//! Authenticated interactive-user attachment, scoped to one owned session.
//! Models use bounded observations and exactly approved, durably recorded input.
mod mutation;
mod router;
mod service;
mod tools;

pub(crate) use router::ComputerRouter;
pub use service::computer_runtime_service;
pub(crate) use tools::tools;

use crate::Daemon;
use agent_computer::{DesktopCapabilities, DesktopSession};
use agent_computer_runtime::RunBinding;
use tokio_util::sync::CancellationToken;
use tonic::Status;

#[derive(Clone)]
pub struct ComputerRuntimeInfo {
  pub host_id: String,
  pub daemon_session_id: String,
  pub owner_epoch: u64,
  pub desktop_session: DesktopSession,
}

impl Daemon {
  pub async fn computer_runtime_info(
    &self,
    session_id: &str,
  ) -> Result<Option<ComputerRuntimeInfo>, Status> {
    let epoch = self.computer_session_epoch(session_id)?;
    Ok(self.computer.info(session_id, epoch).await)
  }

  /// Read-only native permission inspection, fenced by an active owned Run.
  /// No capture, permission prompt, input or lease acquisition occurs.
  pub async fn computer_capabilities(
    &self,
    binding: RunBinding,
    cancellation: CancellationToken,
  ) -> Result<DesktopCapabilities, Status> {
    self.validate_computer_run(&binding)?;
    let result = self
      .computer
      .capabilities(binding.clone(), cancellation)
      .await?;
    self.validate_computer_run(&binding)?;
    Ok(result)
  }

  fn computer_session_epoch(&self, session_id: &str) -> Result<u64, Status> {
    if self.stopping.is_cancelled() {
      return Err(Status::unavailable("daemon is stopping"));
    }
    let session = self
      .store
      .session(session_id)
      .map_err(|_| Status::internal("session storage unavailable"))?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id {
      return Err(Status::permission_denied(
        "session is owned by another node",
      ));
    }
    Ok(session.owner_epoch)
  }

  fn validate_computer_run(&self, binding: &RunBinding) -> Result<(), Status> {
    binding
      .validate()
      .map_err(|_| Status::invalid_argument("invalid run binding"))?;
    if self.computer_session_epoch(&binding.daemon_session_id)? != binding.owner_epoch {
      return Err(Status::failed_precondition("session owner epoch changed"));
    }
    let run = self
      .store
      .run(&binding.run_id)
      .map_err(|_| Status::internal("run storage unavailable"))?
      .ok_or_else(|| Status::not_found("run not found"))?;
    if run.session_id != binding.daemon_session_id
      || !matches!(run.status.as_str(), "running" | "awaiting_approval")
    {
      return Err(Status::failed_precondition(
        "run is not active in this session",
      ));
    }
    Ok(())
  }
}
