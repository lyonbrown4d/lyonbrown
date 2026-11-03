//! Authenticated, owner-fenced commands for the existing Runtime input queue.
mod snapshot;
use crate::Daemon;
use agent_core::events::{AGENT_EVENT_KIND, AgentEvent, QueueKind, QueueStatus};
use agent_protocol::v1;
use agent_runtime::{Runtime, RuntimeError, SessionMutation, SessionPersistence};
use std::sync::Arc;
use tonic::Status;

#[derive(Clone)]
pub(crate) struct RunRuntime {
  pub run_id: String,
  pub epoch: u64,
  pub runtime: Arc<Runtime>,
}

pub(crate) struct RunPersistence {
  pub store: agent_storage::Store,
  pub private_store: Arc<agent_secrets::SecretStore>,
  pub run_id: String,
}
#[async_trait::async_trait]
impl SessionPersistence for RunPersistence {
  async fn load_private_scope(
    &self,
    session: &str,
  ) -> agent_runtime::Result<Option<agent_secrets::SecretString>> {
    let record = self
      .store
      .load_private_scope(session)
      .await
      .map_err(|_| RuntimeError::Persistence("private session state unavailable".into()))?;
    let Some(record) = record else {
      return Ok(None);
    };
    self
      .private_store
      .open_value(&crate::privacy::scope_record_id(session), &record)
      .map(Some)
      .map_err(|_| RuntimeError::Persistence("private session state authentication failed".into()))
  }
  async fn commit_private_scope(
    &self,
    session: &str,
    epoch: u64,
    scope: agent_secrets::SecretString,
  ) -> agent_runtime::Result<()> {
    let record = self
      .private_store
      .seal_value(&crate::privacy::scope_record_id(session), &scope)
      .map_err(|_| RuntimeError::Persistence("private session state encryption failed".into()))?;
    self
      .store
      .commit_private_scope(session, epoch, &record)
      .await
      .map_err(|_| RuntimeError::Persistence("private session state commit unavailable".into()))
  }
  async fn load(&self, session: &str) -> agent_runtime::Result<Option<Vec<u8>>> {
    self
      .store
      .load_snapshot(session)
      .await
      .map_err(|_| RuntimeError::Persistence("session snapshot unavailable".into()))
  }
  async fn commit(
    &self,
    session: &str,
    epoch: u64,
    snapshot: Vec<u8>,
    mut mutation: SessionMutation,
  ) -> agent_runtime::Result<()> {
    // Public events use the daemon Run identity; Rig's internal segment IDs
    // remain private snapshot bookkeeping.
    for event in &mut mutation.events {
      if let AgentEvent::RunFinished { run_id, .. } = event {
        *run_id = self.run_id.clone();
      }
    }
    self
      .store
      .commit_run_snapshot_with_events(
        session,
        epoch,
        Some(&self.run_id),
        &snapshot,
        &mutation.kind,
        mutation.payload,
        &mutation.events,
      )
      .await
      .map_err(|_| RuntimeError::Persistence("session commit could not be confirmed".into()))
  }
}
impl Daemon {
  async fn input_session(
    &self,
    run_id: &str,
    epoch: Option<u64>,
  ) -> Result<(agent_core::Run, agent_core::Session), Status> {
    let run = self
      .store
      .run(run_id)
      .await
      .map_err(failure)?
      .ok_or_else(|| Status::not_found("run not found"))?;
    let session = self
      .store
      .session(&run.session_id)
      .await
      .map_err(failure)?
      .ok_or_else(|| Status::not_found("session not found"))?;
    if session.owner_node_id != self.node_id
      || epoch.is_some_and(|epoch| epoch != session.owner_epoch)
    {
      return Err(Status::failed_precondition(
        "session owner or epoch changed",
      ));
    }
    Ok((run, session))
  }
  pub(crate) async fn submit_input(
    &self,
    input: v1::RunInputRequest,
  ) -> Result<v1::RunInputReceipt, Status> {
    let _submission = self.submission.lock().await;
    if self.stopping.is_cancelled() || self.draining.load(std::sync::atomic::Ordering::Acquire) {
      return Err(Status::unavailable("daemon is draining"));
    }
    let (run, session) = self
      .input_session(&input.run_id, Some(input.owner_epoch))
      .await?;
    if uuid::Uuid::parse_str(&input.request_id).is_err()
      || input.prompt.trim().is_empty()
      || input.prompt.len() > 1_048_576
    {
      return Err(Status::invalid_argument(
        "input requires a UUID request identity and 1..=1MiB prompt",
      ));
    }
    let kind = match v1::RunInputKind::try_from(input.kind).ok() {
      Some(v1::RunInputKind::Steer) => QueueKind::Steer,
      Some(v1::RunInputKind::FollowUp) => QueueKind::FollowUp,
      _ => {
        return Err(Status::invalid_argument(
          "explicit steering or follow-up kind required",
        ));
      }
    };
    let prompt = self
      .protect_private_text(&session.id, input.owner_epoch, &input.prompt)
      .await?;
    let receipts = self.input_receipts(&input.run_id).await?;
    if let Some(receipt) = receipts
      .iter()
      .find(|receipt| receipt.request_id == input.request_id)
    {
      if receipt.run_id != input.run_id || receipt.prompt != prompt || receipt.kind != input.kind {
        return Err(Status::already_exists(
          "input request identity reused with different content",
        ));
      }
      return Ok(receipt.clone());
    }
    if !matches!(run.status.as_str(), "running" | "awaiting_approval") {
      return Err(Status::failed_precondition(
        "run is not accepting inputs; draft was not submitted",
      ));
    }
    if receipts
      .iter()
      .filter(|receipt| receipt.state == v1::RunInputState::Pending as i32)
      .count()
      >= 100
    {
      return Err(Status::resource_exhausted("pending input capacity reached"));
    }
    let pending_bytes: usize = receipts
      .iter()
      .filter(|receipt| receipt.state == v1::RunInputState::Pending as i32)
      .map(|receipt| receipt.prompt.len())
      .sum();
    if pending_bytes.saturating_add(prompt.len()) > 1_048_576 {
      return Err(Status::resource_exhausted(
        "pending input byte capacity reached",
      ));
    }
    let registered = self
      .input_runtimes
      .lock()
      .await
      .get(&session.id)
      .cloned()
      .ok_or_else(|| {
        Status::failed_precondition("runtime is still preparing; input was not accepted")
      })?;
    if registered.run_id != run.id || registered.epoch != session.owner_epoch {
      return Err(Status::failed_precondition("runtime identity changed"));
    }
    if let Err(error) = registered
      .runtime
      .enqueue_input(&input.request_id, kind, &prompt)
      .await
    {
      if matches!(
        error,
        RuntimeError::Persistence(_) | RuntimeError::Uncertain(_)
      ) && let Some(cancel) = self.active.lock().await.get(&run.id)
      {
        cancel.cancel();
      }
      return Err(Status::failed_precondition(
        "input not confirmed; inspect the same request identity before retrying",
      ));
    }
    self.events.notify_waiters();
    self
      .input_receipts(&input.run_id)
      .await?
      .into_iter()
      .find(|receipt| receipt.request_id == input.request_id)
      .ok_or_else(|| {
        Status::internal("input acknowledgement unavailable; inspect the same identity")
      })
  }
  pub(crate) async fn cancel_input(
    &self,
    input: v1::CancelRunInputRequest,
  ) -> Result<v1::RunInputReceipt, Status> {
    let _submission = self.submission.lock().await;
    let (_run, session) = self
      .input_session(&input.run_id, Some(input.owner_epoch))
      .await?;
    let receipt = self
      .input_receipts(&input.run_id)
      .await?
      .into_iter()
      .find(|receipt| receipt.entry_id == input.entry_id)
      .ok_or_else(|| Status::not_found("input entry not found"))?;
    if matches!(
      v1::RunInputState::try_from(receipt.state).ok(),
      Some(v1::RunInputState::Consumed | v1::RunInputState::Cancelled)
    ) {
      return Ok(receipt);
    }
    let registered = self
      .input_runtimes
      .lock()
      .await
      .get(&session.id)
      .cloned()
      .ok_or_else(|| {
        Status::failed_precondition(
          "runtime no longer attached; pending input remains durable and requires inspection",
        )
      })?;
    if registered.epoch != session.owner_epoch {
      return Err(Status::failed_precondition("runtime identity changed"));
    }
    // A new run may have loaded the snapshot while its runtime is still
    // preparing. Never mutate the old runtime and acknowledge a cancellation
    // that the newly loaded queue could otherwise overwrite or consume.
    if let Some(current) = self
      .store
      .runs(&session.id)
      .await
      .map_err(failure)?
      .iter()
      .find(|run| {
        matches!(
          run.status.as_str(),
          "accepted" | "running" | "awaiting_approval" | "cancel_requested"
        )
      })
      && current.id != registered.run_id
    {
      return Err(Status::failed_precondition(
        "current runtime is preparing; cancellation was not accepted",
      ));
    }
    let kind = match v1::RunInputKind::try_from(receipt.kind).ok() {
      Some(v1::RunInputKind::Steer) => QueueKind::Steer,
      Some(v1::RunInputKind::FollowUp) => QueueKind::FollowUp,
      _ => return Err(Status::data_loss("invalid stored input kind")),
    };
    self
      .store
      .append_event_owned(
        &registered.run_id,
        session.owner_epoch,
        AGENT_EVENT_KIND,
        serde_json::to_value(AgentEvent::QueueUpdated {
          entry_id: input.entry_id.clone(),
          kind,
          status: QueueStatus::CancelRequested,
          prompt: None,
          request_id: None,
        })
        .map_err(|_| Status::internal("input cancellation intent unavailable"))?,
      )
      .await
      .map_err(failure)?;
    match registered.runtime.cancel_input(&input.entry_id).await {
      Ok(true) => {}
      Ok(false)
        if registered
          .runtime
          .input_consumed(&input.entry_id)
          .await
          .unwrap_or(false) =>
      {
        self
          .store
          .append_event_owned(
            &registered.run_id,
            session.owner_epoch,
            AGENT_EVENT_KIND,
            serde_json::to_value(AgentEvent::QueueUpdated {
              entry_id: input.entry_id.clone(),
              kind,
              status: QueueStatus::Consumed,
              prompt: None,
              request_id: None,
            })
            .map_err(|_| Status::internal("input consumption confirmation unavailable"))?,
          )
          .await
          .map_err(failure)?;
      }
      _ => {
        if let Some(cancel) = self.active.lock().await.get(&registered.run_id) {
          cancel.cancel();
        }
        if self
          .store
          .fence_run_input_failure(&registered.run_id, session.owner_epoch)
          .await
          .is_err()
        {
          tracing::warn!(operation="run.input.cancel",outcome="unconfirmed",run_id=%registered.run_id,"Durable cancellation intent remains unresolved; session admission is fenced");
        }
        self.events.notify_waiters();
        return Err(Status::failed_precondition(
          "input cancellation was not durably confirmed; session requires reconciliation",
        ));
      }
    }
    self.events.notify_waiters();
    if !registered
      .runtime
      .has_pending_inputs()
      .await
      .unwrap_or(true)
      && !self.active.lock().await.contains_key(&registered.run_id)
    {
      self.input_runtimes.lock().await.remove(&session.id);
    }
    self
      .input_receipts(&input.run_id)
      .await?
      .into_iter()
      .find(|receipt| receipt.entry_id == input.entry_id)
      .ok_or_else(|| Status::internal("input state unavailable"))
  }
}
fn failure(_: anyhow::Error) -> Status {
  Status::internal("input state unavailable")
}
