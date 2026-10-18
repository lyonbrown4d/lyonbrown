use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::{
  ActionOutcome, DesktopAction, DesktopCapabilities, DesktopError, DesktopObservation,
  DesktopSession, DesktopSnapshot, ObservationLimits, lease::LeaseExecution,
};

#[derive(Clone)]
pub struct OperationControl {
  pub cancellation: CancellationToken,
  pub deadline: Instant,
}

impl OperationControl {
  pub fn checkpoint(&self, now: Instant) -> Result<(), DesktopError> {
    if self.cancellation.is_cancelled() {
      return Err(DesktopError::Cancelled);
    }
    if now >= self.deadline {
      return Err(DesktopError::DeadlineExceeded);
    }
    Ok(())
  }
}

pub struct ExecutionGuard {
  pub(crate) lease: LeaseExecution,
  pub(crate) control: OperationControl,
}

impl ExecutionGuard {
  /// Check immediately before each OS side effect. A blocking OS call already
  /// started cannot be recalled; interruption then produces Uncertain.
  pub fn checkpoint(&self) -> Result<(), DesktopError> {
    self.control.checkpoint(self.lease.registry.now())?;
    self.lease.registry.validate(&self.lease.token)
  }
}

/// Platform handles stay behind this port. Implementations must not request OS
/// permissions, infer approvals, retry uncertain input or synthesize screenshots.
pub trait DesktopBackend: Send {
  fn capabilities(&mut self) -> Result<DesktopCapabilities, DesktopError>;
  fn observe(
    &mut self,
    session: &DesktopSession,
    limits: ObservationLimits,
    control: &OperationControl,
  ) -> Result<DesktopObservation, DesktopError>;
  /// Recheck session and complete observation content before input, using the
  /// original capture limits. Snapshot callers cannot weaken those bounds.
  fn validate_snapshot(&mut self, snapshot: &DesktopSnapshot) -> Result<(), DesktopError>;
  fn execute(
    &mut self,
    action: &DesktopAction,
    snapshot: &DesktopSnapshot,
    guard: &ExecutionGuard,
  ) -> ActionOutcome;
}
