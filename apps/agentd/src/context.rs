//! Session-owned maintenance jobs never resume an AgentRun or replay a tool.
mod abandon;
mod binding;
mod jobs;
use crate::{Daemon, run_input::RunPersistence, service::db_error};
use agent_core::context_maintenance::{
  ContextMaintenanceJob as Job, ContextMaintenanceKind as Kind, ContextMaintenanceState as State,
};
use agent_protocol::v1;
use agent_runtime::{Runtime, RuntimeConfig, SessionPersistence};
use std::sync::Arc;
use tonic::Status;
impl Daemon {
  pub(crate) async fn inspect_session_context(
    &self,
    id: &str,
  ) -> Result<v1::ContextStatus, Status> {
    let (configured, session, run_id) = self.context_binding(id).await?;
    let job = self
      .store
      .latest_context_maintenance(id)
      .await
      .map_err(db_error)?;
    // Drop the collection lock before awaiting Runtime state or persistence.
    let active = { self.input_runtimes.lock().await.get(id).cloned() };
    let retained_runtime = active.is_some();
    let mut status = if let Some(active) = active {
      context_view(
        active
          .runtime
          .context_status()
          .await
          .map_err(runtime_error)?,
      )
    } else {
      let config = configured.context_config(&session)?;
      context_view(
        agent_runtime::inspect_context(&config, self.context_persistence(run_id), &[])
          .await
          .map_err(runtime_error)?,
      )
    };
    let active_run = self
      .store
      .runs(id)
      .await
      .map_err(db_error)?
      .iter()
      .any(|run| {
        matches!(
          run.status.as_str(),
          "accepted" | "running" | "awaiting_approval" | "cancel_requested" | "uncertain"
        )
      });
    let maintained = job.as_ref().is_some_and(|job| job.state.blocks_admission());
    status.manual_operations_available =
      !active_run && !maintained && !retained_runtime && !self.stopping.is_cancelled();
    status.summary_pipeline_available = true;
    status.can_undo &= status.manual_operations_available;
    status.summary_request_pending |= maintained;
    status.maintenance = job.map(job_view);
    Ok(status)
  }
  pub(crate) async fn inspect_session_summary(
    &self,
    id: &str,
  ) -> Result<v1::ContextSummary, Status> {
    let (configured, session, run_id) = self.context_binding(id).await?;
    let config = configured.context_config(&session)?;
    let (revision, text, truncated) =
      agent_runtime::inspect_summary(&config, self.context_persistence(run_id))
        .await
        .map_err(runtime_error)?;
    Ok(v1::ContextSummary {
      revision,
      text,
      truncated,
    })
  }
  pub(crate) async fn mutate_context(
    &self,
    session_id: &str,
    epoch: u64,
    revision: u64,
    focus: Option<&str>,
  ) -> Result<v1::ContextStatus, Status> {
    if focus.is_some_and(|focus| focus.len() > 4096) {
      return Err(Status::invalid_argument("focus exceeds 4096 bytes"));
    }
    let submission = self.submission.lock().await;
    if self.stopping.is_cancelled() || self.draining.load(std::sync::atomic::Ordering::Acquire) {
      return Err(Status::unavailable("daemon is stopping or draining"));
    }
    let (configured, session, run_id) = self.context_binding(session_id).await?;
    if session.owner_epoch != epoch {
      return Err(Status::aborted("session epoch changed"));
    }
    if self.input_runtimes.lock().await.contains_key(session_id) {
      return Err(Status::failed_precondition(
        "reconcile retained queued input before detached context maintenance",
      ));
    }
    if self.context_jobs.lock().await.len() >= self.config.max_runs {
      return Err(Status::resource_exhausted(
        "context maintenance capacity reached",
      ));
    }
    let config = configured.context_config(&session)?;
    let initial =
      agent_runtime::inspect_context(&config, self.context_persistence(run_id.clone()), &[])
        .await
        .map_err(runtime_error)?;
    if initial.revision != revision {
      return Err(Status::aborted("context revision changed"));
    }
    if self.stopping.is_cancelled() {
      return Err(Status::unavailable("daemon is stopping"));
    }
    let kind = if focus.is_some() {
      Kind::Compact
    } else {
      Kind::Undo
    };
    let job = self
      .store
      .begin_context_maintenance(session_id, &run_id, epoch, revision, kind)
      .await
      .map_err(db_error)?;
    let cancel = self.stopping.child_token();
    self
      .context_jobs
      .lock()
      .await
      .insert(job.id.clone(), cancel.clone());
    drop(submission);
    self.events.notify_waiters();
    if let Some(focus) = focus {
      let daemon = self.clone();
      let focus = focus.to_owned();
      let queued = job.clone();
      tokio::spawn(async move {
        daemon
          .execute_context_job(configured, config, queued, focus, cancel)
          .await;
      });
      let mut status = context_view(initial);
      status.summary_request_pending = true;
      status.can_undo = false;
      status.maintenance = Some(job_view(job));
      return Ok(status);
    }
    // A detached owner completes even if the unary RPC is dropped mid-commit.
    let daemon = self.clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
      let result = daemon.execute_context_undo(config, job, cancel).await;
      let _ = send.send(result);
    });
    receive.await.map_err(|_| {
      Status::unavailable("context undo result unavailable; inspect maintenance state")
    })?
  }
  pub(crate) async fn cancel_context_job(
    &self,
    id: &str,
    epoch: u64,
  ) -> Result<v1::ContextMaintenanceJob, Status> {
    uuid::Uuid::parse_str(id).map_err(|_| Status::invalid_argument("job UUID required"))?;
    let job = self
      .store
      .context_maintenance_job(id)
      .await
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("context job not found"))?;
    let session = self
      .store
      .session(&job.session_id)
      .await
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id
      || session.owner_epoch != epoch
      || job.owner_epoch != epoch
    {
      return Err(Status::aborted("context maintenance owner changed"));
    }
    let job = self
      .store
      .request_context_cancellation(id, epoch)
      .await
      .map_err(db_error)?;
    if let Some(cancel) = self.context_jobs.lock().await.get(id).cloned() {
      cancel.cancel();
    }
    self.events.notify_waiters();
    Ok(job_view(job))
  }
}
fn runtime_error(_: agent_runtime::RuntimeError) -> Status {
  Status::failed_precondition(
    "context command rejected; inspect revision, model limits, privacy preparation and maintenance state",
  )
}
fn context_view(s: agent_runtime::ContextStatus) -> v1::ContextStatus {
  v1::ContextStatus {
    revision: s.revision,
    message_count: s.message_count as u64,
    compaction_count: s.compaction_count as u64,
    has_summary: s.has_summary,
    can_undo: s.can_undo,
    estimated_input_tokens: s.estimated_input_tokens,
    estimate_includes_tools: s.estimate_includes_tools,
    context_window: s.context_window,
    reserved_output_tokens: s.reserved_output_tokens,
    summary_request_pending: s.summary_request_pending,
    summary_pipeline_available: s.summary_pipeline_available,
    manual_operations_available: false,
    maintenance: None,
  }
}
fn job_view(job: Job) -> v1::ContextMaintenanceJob {
  v1::ContextMaintenanceJob {
    id: job.id,
    session_id: job.session_id,
    run_id: job.run_id,
    owner_epoch: job.owner_epoch,
    expected_revision: job.expected_revision,
    cancel_requested: job.cancel_requested,
    abandon_reason: job.abandon_reason,
    undo: job.kind == Kind::Undo,
    created_at_ms: job.created_at_ms,
    updated_at_ms: job.updated_at_ms,
    state: match job.state {
      State::Preparing => v1::ContextMaintenanceState::Preparing,
      State::Sending => v1::ContextMaintenanceState::Sending,
      State::Completed => v1::ContextMaintenanceState::Completed,
      State::Failed => v1::ContextMaintenanceState::Failed,
      State::Cancelled => v1::ContextMaintenanceState::Cancelled,
      State::Uncertain => v1::ContextMaintenanceState::Uncertain,
      State::Abandoned => v1::ContextMaintenanceState::Abandoned,
    } as i32,
  }
}
