use super::*;
impl Daemon {
  pub(crate) async fn inspect_context_job(
    &self,
    session_id: &str,
  ) -> Result<v1::ContextMaintenanceJob, Status> {
    let session = self
      .store
      .session(session_id)
      .await
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id {
      return Err(Status::permission_denied("session has a different owner"));
    }
    self
      .store
      .latest_context_maintenance(session_id)
      .await
      .map_err(db_error)?
      .map(job_view)
      .ok_or_else(|| Status::not_found("context maintenance job not found"))
  }
  pub(crate) async fn abandon_context_job(
    &self,
    input: v1::AbandonContextRequest,
  ) -> Result<v1::AbandonContextResult, Status> {
    uuid::Uuid::parse_str(&input.job_id)
      .map_err(|_| Status::invalid_argument("job UUID required"))?;
    let _submission = self.submission.lock().await;
    let job = self
      .store
      .context_maintenance_job(&input.job_id)
      .await
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("context job not found"))?;
    if self.context_jobs.lock().await.contains_key(&job.id) {
      return Err(Status::failed_precondition(
        "cancel and wait for the context worker to stop before resolving its outcome",
      ));
    }
    let session = self
      .store
      .session(&job.session_id)
      .await
      .map_err(db_error)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id {
      return Err(Status::permission_denied("session has a different owner"));
    }
    let (job, session) = self
      .store
      .abandon_context_maintenance(
        &input.job_id,
        input.owner_epoch,
        &input.request_id,
        &input.reason,
      )
      .await
      .map_err(db_error)?;
    self.events.notify_waiters();
    Ok(v1::AbandonContextResult {
      job: Some(job_view(job)),
      session: Some(session.into()),
      new_session_required: true,
    })
  }
}
