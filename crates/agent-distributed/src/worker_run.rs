use std::{path::Path, time::Duration};

use anyhow::{Result, ensure};
use tokio_util::sync::CancellationToken;

use crate::worker::{TaskWorker, WorkerJob};

impl TaskWorker {
  pub(crate) fn launch(&self, job: WorkerJob, cancellation: CancellationToken) {
    let worker = self.clone();
    tokio::spawn(async move {
      let task = &job.task;
      let result = worker.run_job(job.clone(), cancellation.clone()).await;
      let update = match result {
        Ok(Some(Ok(output))) => worker.manager.store.finish_attempt(
          &task.task_id,
          &task.attempt_id,
          task.owner_epoch,
          &output,
          "",
        ),
        Ok(Some(Err(error))) if error.is::<crate::worker::UncertainOutcome>() => worker
          .manager
          .store
          .mark_task_uncertain(&task.task_id, &task.attempt_id, task.owner_epoch),
        Ok(Some(Err(_))) if cancellation.is_cancelled() => worker
          .manager
          .store
          .mark_task_cancelled(&task.task_id, &task.attempt_id, task.owner_epoch),
        Ok(Some(Err(_))) => worker.manager.store.finish_attempt(
          &task.task_id,
          &task.attempt_id,
          task.owner_epoch,
          "",
          "worker execution failed; inspect worker logs",
        ),
        Ok(None) | Err(_) => worker.manager.store.mark_task_uncertain(
          &task.task_id,
          &task.attempt_id,
          task.owner_epoch,
        ),
      };
      if update.is_err() {
        tracing::error!(task_id = %task.task_id,
          operation = "worker.finish",
          outcome = "state_write_failed",
          "worker outcome requires reconciliation"
        );
      }
      worker.active.lock().await.remove(&task.task_id);
      worker.changed.notify_waiters();
    });
  }

  async fn run_job(
    &self,
    job: WorkerJob,
    cancellation: CancellationToken,
  ) -> Result<Option<Result<String>>> {
    self.manager.store.mark_task_running(
      &job.task.task_id,
      &job.task.attempt_id,
      job.task.owner_epoch,
    )?;
    create_workspace(&job.workspace)?;
    let owner = job.task.owner_node_id.clone();
    let owner_fingerprint = self.manager.trust(&owner)?.fingerprint;
    let remaining = job
      .task
      .deadline_ms
      .saturating_sub(agent_core::now_ms())
      .max(0) as u64;
    let deadline = tokio::time::sleep(Duration::from_millis(remaining));
    tokio::pin!(deadline);
    let execution = self.executor.execute(job, cancellation.clone());
    tokio::pin!(execution);
    let mut trust_check = tokio::time::interval(Duration::from_millis(250));
    loop {
      tokio::select! {
        result = &mut execution => return Ok(Some(result)),
        _ = cancellation.cancelled() => break,
        _ = &mut deadline => { cancellation.cancel(); break; },
        _ = trust_check.tick() => {
          if !self.manager.trust(&owner).is_ok_and(|trust| trust.fingerprint == owner_fingerprint) {
            cancellation.cancel(); break;
          }
        }
      }
    }
    // Cancellation is cooperative. A missing confirmation is an unknown outcome.
    match tokio::time::timeout(Duration::from_secs(15), execution).await {
      Ok(result) => Ok(Some(result)),
      Err(_) => Ok(None),
    }
  }
}

fn create_workspace(path: &Path) -> Result<()> {
  let parent = path
    .parent()
    .ok_or_else(|| anyhow::anyhow!("invalid attempt workspace"))?;
  match std::fs::create_dir(parent) {
    Ok(()) => {}
    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
      let metadata = std::fs::symlink_metadata(parent)?;
      ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "task workspace is not a private directory"
      );
    }
    Err(error) => return Err(error.into()),
  }
  std::fs::create_dir(path)?;
  Ok(())
}
