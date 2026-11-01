use crate::Daemon;
use agent_model_hub::{HubClient, InstallPlan};
impl Daemon {
  pub(crate) async fn start_model_install(
    &self,
    client: HubClient,
    plan: InstallPlan,
  ) -> anyhow::Result<serde_json::Value> {
    let id = plan.id.clone();
    let value = serde_json::json!({"id":id,"state":"accepted","plan":plan});
    let mut jobs = self.model_jobs.lock().await;
    if jobs.contains_key(&id) {
      return Ok(serde_json::json!({"id":id,"state":"running"}));
    }
    self.store.record_model_job(&id, value.clone())?;
    let token = self.stopping.child_token();
    jobs.insert(id.clone(), token.clone());
    let daemon = self.clone();
    tokio::spawn(async move {
      let (send, mut progress) = tokio::sync::mpsc::channel(32);
      let store = daemon.store.clone();
      let progress_id = id.clone();
      let progress_cancel = token.clone();
      let watcher = tokio::spawn(async move {
        while let Some(value) = progress.recv().await {
          if let Ok(value) = serde_json::to_value(value)
            && store.record_model_job(&progress_id, value).is_err()
          {
            progress_cancel.cancel();
            break;
          }
        }
      });
      let result = client.install(&plan, &token, Some(send)).await;
      let _ = watcher.await;
      let final_state = match result {
        Ok(model) => serde_json::json!({"id":id,"state":"installed_unverified","model":model}),
        Err(_) => {
          serde_json::json!({"id":id,"state":if token.is_cancelled(){"cancelled"}else{"failed"},"reason":"Model installation did not complete; check source, credentials and disk space"})
        }
      };
      if daemon.store.record_model_job(&id, final_state).is_err() {
        tracing::error!(job_id=%id,"model install completion could not be persisted; reconcile uncertain on restart");
      }
      daemon.model_jobs.lock().await.remove(&id);
      daemon.events.notify_waiters();
    });
    Ok(value)
  }
}
