use super::{
  CommandAuthorization, CommandOutcome, CommandPolicy, CommandRequest, CommandStatus,
  CommandStopReason, platform, supervise,
};
use anyhow::{Context, Result, ensure};
use std::{
  path::{Path, PathBuf},
  sync::Arc,
  time::Duration,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct CommandExecutor {
  root: PathBuf,
  policy: CommandPolicy,
  authorization: Arc<dyn CommandAuthorization>,
}
impl CommandExecutor {
  /// `root` must already have been authorized by the daemon.
  pub fn open(
    root: impl AsRef<Path>,
    policy: CommandPolicy,
    authorization: Arc<dyn CommandAuthorization>,
  ) -> Result<Self> {
    let root = root
      .as_ref()
      .canonicalize()
      .context("resolve authorized workspace")?;
    ensure!(root.is_dir(), "workspace must be a directory");
    Ok(Self {
      root,
      policy,
      authorization,
    })
  }

  pub async fn execute(
    &self,
    request: CommandRequest,
    cancellation: CancellationToken,
  ) -> CommandOutcome {
    use CommandStatus as Status;
    use CommandStopReason as Reason;
    if cancellation.is_cancelled() {
      return CommandOutcome::stopped(&request, Status::CancelledBeforeStart, Reason::Cancelled);
    }
    let resolved = match self.policy.resolve(&self.root, &request) {
      Ok(resolved) => resolved,
      Err(_) => return CommandOutcome::stopped(&request, Status::Rejected, Reason::InvalidRequest),
    };
    if !cfg!(any(unix, windows)) {
      return CommandOutcome::stopped(&request, Status::Rejected, Reason::UnsupportedPlatform);
    }
    let deadline = Instant::now() + Duration::from_millis(request.timeout_ms);
    let approval = tokio::select! {
      biased;
      _ = cancellation.cancelled() => return CommandOutcome::stopped(&request, Status::CancelledBeforeStart, Reason::Cancelled),
      _ = tokio::time::sleep_until(deadline) => return CommandOutcome::stopped(&request, Status::Rejected, Reason::Deadline),
      result = self.authorization.authorize(&request, &resolved) => result,
    };
    match approval {
      Ok(true) => {}
      Ok(false) => return CommandOutcome::stopped(&request, Status::Denied, Reason::Unauthorized),
      Err(_) => {
        return CommandOutcome::stopped(&request, Status::Denied, Reason::AuthorizationFailed);
      }
    }
    // Approval may take time: resolve again before dispatch, and never silently change its scope.
    if self.policy.resolve(&self.root, &request).ok().as_ref() != Some(&resolved) {
      return CommandOutcome::stopped(&request, Status::Rejected, Reason::InvalidRequest);
    }
    if cancellation.is_cancelled() {
      return CommandOutcome::stopped(&request, Status::CancelledBeforeStart, Reason::Cancelled);
    }
    if Instant::now() >= deadline {
      return CommandOutcome::stopped(&request, Status::Rejected, Reason::Deadline);
    }
    let process = match platform::spawn(&self.policy, &request, &resolved) {
      Ok(process) => process,
      // A platform wrapper may fail after creating a process. Do not claim zero effects.
      Err(_) => return CommandOutcome::stopped(&request, Status::Uncertain, Reason::SpawnFailed),
    };
    supervise::run(process, &request, cancellation, deadline).await
  }
}
