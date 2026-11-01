use std::sync::Arc;

use agent_protocol::v1;
use agent_storage::{RecoveryAction, RunRecovery};
use tonic::Status;

use crate::{Daemon, service::db_error};

#[cfg(test)]
#[path = "../tests/recovery_support/writer.rs"]
mod tests;

fn action(value: RecoveryAction) -> v1::RunRecoveryAction {
  match value {
    RecoveryAction::Abandon => v1::RunRecoveryAction::Abandon,
    RecoveryAction::ContinueNewSession => v1::RunRecoveryAction::ContinueNewSession,
  }
}

impl Daemon {
  fn recovery_result(&self, resolution: RunRecovery) -> Result<v1::RunRecoveryResult, Status> {
    Ok(v1::RunRecoveryResult {
      original_run: self
        .store
        .run(&resolution.run_id)
        .map_err(db_error)?
        .map(Into::into),
      session: resolution
        .session_id
        .as_deref()
        .map(|id| self.store.session(id))
        .transpose()
        .map_err(db_error)?
        .flatten()
        .map(Into::into),
      run: resolution
        .replacement_run_id
        .as_deref()
        .map(|id| self.store.run(id))
        .transpose()
        .map_err(db_error)?
        .flatten()
        .map(Into::into),
      action: action(resolution.action).into(),
      note: resolution.note,
      owner_epoch: resolution.requested_owner_epoch,
      idempotency_key: resolution.idempotency_key,
    })
  }

  pub(crate) async fn inspect_run_recovery(&self, id: &str) -> Result<v1::RunRecovery, Status> {
    let run = self
      .store
      .run(id)
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("run not found"))?;
    let session = self
      .store
      .session(&run.session_id)
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    let resolution = self
      .store
      .run_recovery(id)
      .map_err(db_error)?
      .map(|resolution| self.recovery_result(resolution))
      .transpose()?;
    let report = agent_runtime::inspect_recovery(
      &session.id,
      session.owner_epoch,
      Arc::new(self.store.clone()),
    )
    .await;
    let mut reason = run.error.clone();
    let pending_tools = match report {
      Ok(report) => {
        if let Some(uncertainty) = report.uncertainty {
          if !reason.is_empty() {
            reason.push_str("; ");
          }
          reason.push_str(&uncertainty);
        }
        report
          .pending_tools
          .into_iter()
          .take(128)
          .map(|tool| tool.chars().take(512).collect())
          .collect()
      }
      Err(error) => {
        if !reason.is_empty() {
          reason.push_str("; ");
        }
        reason.push_str(&format!("snapshot inspection failed: {error}"));
        Vec::new()
      }
    };
    let unresolved_tasks = self
      .store
      .unresolved_recovery_tasks(&session.id)
      .map_err(db_error)?;
    if unresolved_tasks > 0 {
      reason
        .push_str("; reconcile and cancel or accept/reject unresolved Tasks before Run recovery");
    }
    let available = run.status == "uncertain"
      && unresolved_tasks == 0
      && resolution.is_none()
      && session.owner_node_id == self.node_id
      && !session.id.starts_with("worker:")
      && !self.active.lock().await.contains_key(id);
    Ok(v1::RunRecovery {
      run: Some(run.into()),
      owner_epoch: session.owner_epoch,
      reason: reason.chars().take(4096).collect(),
      pending_tools,
      allowed_actions: if available {
        vec![
          v1::RunRecoveryAction::Abandon.into(),
          v1::RunRecoveryAction::ContinueNewSession.into(),
        ]
      } else {
        Vec::new()
      },
      resolution,
      fresh_session_required: true,
    })
  }

  pub(crate) async fn resolve_run_recovery(
    &self,
    input: v1::RecoverRunRequest,
  ) -> Result<v1::RunRecoveryResult, Status> {
    let _submission = self.submission.lock().await;
    if self.stopping.is_cancelled() || self.draining.load(std::sync::atomic::Ordering::Acquire) {
      return Err(Status::unavailable("daemon is shutting down"));
    }
    let requested = match v1::RunRecoveryAction::try_from(input.action) {
      Ok(v1::RunRecoveryAction::Abandon) => RecoveryAction::Abandon,
      Ok(v1::RunRecoveryAction::ContinueNewSession) => RecoveryAction::ContinueNewSession,
      _ => {
        return Err(Status::invalid_argument(
          "choose an explicit recovery action",
        ));
      }
    };
    // Detached blocking file writers retain this guard until their filesystem
    // operation finishes. Wait before recording the human decision and fence.
    let writes = self.workspace_writes.clone().lock_owned().await;
    let active = self.active.lock().await;
    if active.contains_key(&input.run_id) {
      return Err(Status::failed_precondition("run execution has not stopped"));
    }
    if requested == RecoveryAction::ContinueNewSession && active.len() >= 128 {
      return Err(Status::resource_exhausted("node run capacity reached"));
    }
    let (resolution, created) = self
      .store
      .recover_run(agent_storage::RecoverRun {
        run_id: &input.run_id,
        owner_epoch: input.owner_epoch,
        node_id: &self.node_id,
        action: requested,
        idempotency_key: &input.idempotency_key,
        note: &input.note,
        prompt: &input.prompt,
      })
      .map_err(db_error)?;
    drop(active);
    drop(writes);
    if created && let Some(id) = &resolution.replacement_run_id {
      let run = self
        .store
        .run(id)
        .map_err(db_error)?
        .ok_or_else(|| Status::internal("recovery execution intent missing"))?;
      self.launch(run).await.map_err(db_error)?;
    }
    self.events.notify_waiters();
    self.recovery_result(resolution)
  }
}
