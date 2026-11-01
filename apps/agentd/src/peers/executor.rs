use std::collections::BTreeSet;

use agent_core::RunStatus;
use agent_distributed::worker::{UncertainOutcome, WorkerExecutor, WorkerJob};
use anyhow::{Result, ensure};
use tokio_util::sync::CancellationToken;

use crate::Daemon;

pub(super) struct HarnessWorker(pub Daemon);

#[async_trait::async_trait]
impl WorkerExecutor for HarnessWorker {
  async fn execute(&self, job: WorkerJob, cancellation: CancellationToken) -> Result<String> {
    let daemon = &self.0;
    ensure!(
      job.session.owner_node_id == daemon.node_id,
      "worker session owner differs"
    );
    ensure!(
      std::path::Path::new(&job.session.workspace) == job.workspace && job.workspace.is_dir(),
      "worker session must use its isolated attempt directory"
    );
    ensure!(
      !cancellation.is_cancelled(),
      "worker task cancelled before execution"
    );
    // The remote envelope controls only the prompt and a reduction of tool authority.
    // Provider credentials, endpoint and model always come from this daemon's configuration.
    let permitted = job
      .task
      .capabilities
      .iter()
      .flat_map(|cap| match cap.as_str() {
        "workspace.read" => vec![
          "read_file".to_owned(),
          "list_files".to_owned(),
          "search_files".to_owned(),
        ],
        "workspace.write.approval" => vec!["write_file".to_owned()],
        _ => vec![],
      })
      .collect::<BTreeSet<_>>();
    let (run, created) = daemon.store.accept_run(
      &job.session.id,
      job.session.owner_epoch,
      &job.task.instructions,
      &job.task.attempt_id,
    )?;
    ensure!(
      created,
      "worker run already exists; reconcile without replay"
    );
    daemon
      .store
      .update_run_owned(&run.id, job.session.owner_epoch, RunStatus::Running, "", "")?;
    let permit = tokio::select! {
      value = daemon.permits.clone().acquire_owned() => value.ok(),
      _ = cancellation.cancelled() => None,
      _ = daemon.stopping.cancelled() => None,
    };
    if permit.is_none() {
      cancellation.cancel();
      daemon.store.update_run_owned(
        &run.id,
        job.session.owner_epoch,
        RunStatus::Cancelled,
        "",
        "",
      )?;
      anyhow::bail!("worker task cancelled while queued");
    }
    let result = daemon
      .execute_run_scoped(&run, &job.session, cancellation, Some(&permitted))
      .await;
    drop(permit);
    if let Err(error) = result {
      let uncertain = matches!(
        error.downcast_ref::<agent_runtime::RuntimeError>(),
        Some(
          agent_runtime::RuntimeError::Uncertain(_) | agent_runtime::RuntimeError::Persistence(_)
        )
      ) || error
        .downcast_ref::<agent_runtime::RuntimeError>()
        .is_none();
      daemon
        .store
        .update_run_owned(
          &run.id,
          job.session.owner_epoch,
          if uncertain {
            RunStatus::Uncertain
          } else {
            RunStatus::Failed
          },
          "",
          &error.to_string(),
        )
        .map_err(|_| UncertainOutcome)?;
      if uncertain {
        return Err(UncertainOutcome.into());
      }
      return Err(error);
    }
    let terminal = daemon
      .store
      .run(&run.id)
      .map_err(|_| UncertainOutcome)?
      .ok_or(UncertainOutcome)?;
    match terminal.status.as_str() {
      "completed" => Ok(terminal.result),
      "uncertain" => Err(UncertainOutcome.into()),
      _ => anyhow::bail!("worker Harness did not complete: {}", terminal.status),
    }
  }
}
