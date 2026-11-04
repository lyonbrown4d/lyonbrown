use crate::Daemon;
use agent_core::{Run, RunStatus};
use agent_runtime::RuntimeError;
use anyhow::Result;
use tokio_util::sync::CancellationToken;
impl Daemon {
  pub(crate) async fn launch(&self, run: Run) -> Result<()> {
    let session = self
      .store
      .session(&run.session_id)
      .await?
      .ok_or_else(|| anyhow::anyhow!("session missing"))?;
    let cancel = CancellationToken::new();
    self
      .active
      .lock()
      .await
      .insert(run.id.clone(), cancel.clone());
    let daemon = self.clone();
    tokio::spawn(async move {
      let permit = tokio::select! {value=daemon.permits.clone().acquire_owned()=>value.ok(),_=cancel.cancelled()=>None};
      if permit.is_none() {
        let _ = daemon
          .store
          .update_run_owned(&run.id, session.owner_epoch, RunStatus::Cancelled, "", "")
          .await;
      } else if let Err(error) = daemon
        .execute_run_scoped(&run, &session, cancel, None)
        .await
      {
        let status = if matches!(
          error.downcast_ref::<RuntimeError>(),
          Some(RuntimeError::Uncertain(_) | RuntimeError::Persistence(_))
        ) {
          RunStatus::Uncertain
        } else {
          RunStatus::Failed
        };
        if let Err(storage) = daemon
          .store
          .update_run_owned(
            &run.id,
            session.owner_epoch,
            status,
            "",
            &agent_storage::public_error_message(&error),
          )
          .await
        {
          tracing::error!(run_id=%run.id,error=%agent_storage::public_error_message(&storage),"could not persist failed run");
        }
      }
      drop(permit);
      daemon.active.lock().await.remove(&run.id);
      daemon.events.notify_waiters();
    });
    Ok(())
  }
}
