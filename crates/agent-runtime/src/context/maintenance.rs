//! Pure recovery for host maintenance that durably never reached provider IO.
use crate::{Result, RuntimeError, SessionPersistence, persistence::DurableSession};
use std::sync::Arc;
pub async fn cancel_unsent_summary(
  session_id: &str,
  owner_epoch: u64,
  persistence: Arc<dyn SessionPersistence>,
) -> Result<bool> {
  let session = DurableSession::open(session_id, owner_epoch, persistence).await?;
  let state = session.state.lock().await;
  let Some(intent) = &state.context.pending else {
    return Ok(false);
  };
  if intent.sending {
    return Err(RuntimeError::Uncertain(
      "sent summary requires explicit reconciliation; it cannot be cleared as unsent".into(),
    ));
  }
  drop(state);
  session
    .update("context_summary_cancelled_before_send", |state| {
      if state
        .context
        .pending
        .as_ref()
        .is_some_and(|intent| intent.sending)
      {
        return Err(RuntimeError::Uncertain("summary send state changed".into()));
      }
      state.context.pending = None;
      Ok((
        true,
        serde_json::json!({"outcome":"cancelled_before_send"}),
        vec![],
      ))
    })
    .await
}
