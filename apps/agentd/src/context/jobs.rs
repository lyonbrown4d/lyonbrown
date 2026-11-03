use super::*;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
impl Daemon {
  pub(super) async fn execute_context_job(
    &self,
    configured: Daemon,
    config: RuntimeConfig,
    job: Job,
    focus: String,
    cancel: CancellationToken,
  ) {
    let succeeded = self
      .perform_context_job(&configured, config, &job, &focus, &cancel)
      .await;
    if let Ok(Some(current)) = self.store.context_maintenance_job(&job.id).await {
      if matches!(current.state, State::Preparing | State::Sending) {
        let outcome = if current.state == State::Sending {
          State::Uncertain
        } else {
          match agent_runtime::cancel_unsent_summary(
            &job.session_id,
            job.owner_epoch,
            self.context_persistence(job.run_id.clone()),
          )
          .await
          {
            Err(_) => State::Uncertain,
            Ok(_) if cancel.is_cancelled() || current.cancel_requested => State::Cancelled,
            Ok(_) if succeeded => State::Completed,
            Ok(_) => State::Failed,
          }
        };
        if let Err(error) = self
          .store
          .finish_context_maintenance(&job.id, job.owner_epoch, outcome)
          .await
        {
          tracing::warn!(job_id = %job.id, error_category = "context_receipt", error=%agent_storage::public_error_message(&error), "Context maintenance receipt requires reconciliation");
        }
      }
    }
    self.context_jobs.lock().await.remove(&job.id);
    self.events.notify_waiters();
  }
  pub(super) async fn execute_context_undo(
    &self,
    config: RuntimeConfig,
    job: Job,
    cancel: CancellationToken,
  ) -> Result<v1::ContextStatus, Status> {
    let result = if cancel.is_cancelled() {
      Err(agent_runtime::RuntimeError::Execution(
        "undo cancelled before persistence".into(),
      ))
    } else {
      agent_runtime::undo_context(
        &config,
        self.context_persistence(job.run_id.clone()),
        job.expected_revision,
      )
      .await
    };
    let outcome = match &result {
      Ok(_) => State::Completed,
      Err(
        agent_runtime::RuntimeError::Persistence(_) | agent_runtime::RuntimeError::Uncertain(_),
      ) => State::Uncertain,
      Err(_) if cancel.is_cancelled() => State::Cancelled,
      Err(_) => State::Failed,
    };
    let finished = self
      .store
      .finish_context_maintenance(&job.id, job.owner_epoch, outcome)
      .await;
    self.context_jobs.lock().await.remove(&job.id);
    self.events.notify_waiters();
    let job = finished.map_err(db_error)?;
    let mut status = context_view(result.map_err(runtime_error)?);
    status.manual_operations_available = true;
    status.maintenance = Some(job_view(job));
    Ok(status)
  }
  async fn perform_context_job(
    &self,
    configured: &Daemon,
    config: RuntimeConfig,
    job: &Job,
    focus: &str,
    cancel: &CancellationToken,
  ) -> bool {
    let permit = tokio::select! { biased;
      _ = cancel.cancelled() => return false,
      permit = tokio::time::timeout(Duration::from_secs(30), self.permits.clone().acquire_owned()) => match permit {
        Ok(Ok(permit)) => permit, _ => return false,
      },
    };
    let runtime = match configured.context_runtime(config, job.run_id.clone()).await {
      Ok(runtime) => runtime,
      Err(_) => return false,
    };
    if !runtime
      .context_status()
      .await
      .is_ok_and(|status| status.revision == job.expected_revision)
    {
      return false;
    }
    let result = runtime.compact(focus);
    tokio::pin!(result);
    let completed = tokio::select! { biased;
      _ = cancel.cancelled() => {
        let _ = runtime.cancel().await;
        matches!(tokio::time::timeout(Duration::from_secs(5), &mut result).await, Ok(Ok(_)))
      },
      result = &mut result => result.is_ok(),
    };
    drop(permit);
    completed
  }
  /// Startup inspects host intent only. It never resumes a summary request.
  pub(crate) async fn recover_context_jobs(&self) -> anyhow::Result<()> {
    let jobs = self.store.recover_context_maintenance().await?;
    for job in jobs {
      let Some(session) = self.store.session(&job.session_id).await? else {
        continue;
      };
      if session.owner_node_id != self.node_id || session.owner_epoch != job.owner_epoch {
        continue;
      }
      let outcome = match agent_runtime::cancel_unsent_summary(
        &job.session_id,
        job.owner_epoch,
        self.context_persistence(job.run_id.clone()),
      )
      .await
      {
        Ok(_) => State::Cancelled,
        Err(_) => State::Uncertain,
      };
      self
        .store
        .finish_context_maintenance(&job.id, job.owner_epoch, outcome)
        .await?;
    }
    Ok(())
  }
}
