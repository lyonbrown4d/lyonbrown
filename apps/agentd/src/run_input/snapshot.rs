use super::{Daemon, failure};
use agent_core::events::{AGENT_EVENT_KIND, AgentEvent, QueueKind, QueueStatus};
use agent_protocol::v1;
use std::collections::BTreeMap;
use tonic::Status;
impl Daemon {
  pub(crate) async fn input_receipts(
    &self,
    run_id: &str,
  ) -> Result<Vec<v1::RunInputReceipt>, Status> {
    let (run, _) = self.input_session(run_id, None).await?;
    let mut receipts = BTreeMap::<String, v1::RunInputReceipt>::new();
    for source in self.store.runs(&run.session_id).await.map_err(failure)? {
      let mut cursor = 0;
      loop {
        let batch = self
          .store
          .events(&source.id, cursor, 256)
          .await
          .map_err(failure)?;
        for event in &batch {
          if event.kind != AGENT_EVENT_KIND {
            continue;
          }
          let agent_core::events::DecodedEvent::Known(AgentEvent::QueueUpdated {
            entry_id,
            kind,
            status,
            prompt,
            request_id,
          }) = agent_core::events::decode(&event.kind, &event.payload_json)
            .map_err(|_| Status::data_loss("invalid stored input event"))?
          else {
            continue;
          };
          if status == QueueStatus::Pending {
            if let Some(request_id) = request_id {
              receipts.insert(
                entry_id.clone(),
                v1::RunInputReceipt {
                  run_id: source.id.clone(),
                  request_id,
                  entry_id,
                  kind: match kind {
                    QueueKind::Steer => v1::RunInputKind::Steer,
                    QueueKind::FollowUp => v1::RunInputKind::FollowUp,
                  } as i32,
                  state: v1::RunInputState::Pending as i32,
                  prompt: prompt.unwrap_or_default(),
                  prompt_truncated: false,
                },
              );
            }
          } else if let Some(receipt) = receipts.get_mut(&entry_id) {
            receipt.state = match status {
              QueueStatus::Consumed => v1::RunInputState::Consumed,
              QueueStatus::Cancelled => v1::RunInputState::Cancelled,
              QueueStatus::CancelRequested => v1::RunInputState::CancelRequested,
              QueueStatus::Pending => v1::RunInputState::Pending,
            } as i32;
          }
        }
        if batch.len() < 256 {
          break;
        }
        cursor = batch.last().map_or(cursor, |event| event.cursor);
      }
    }
    Ok(receipts.into_values().collect())
  }
  pub(crate) async fn input_snapshot(&self, run_id: &str) -> Result<v1::RunInputs, Status> {
    let receipts = self.input_receipts(run_id).await?;
    let (mut pending, terminal): (Vec<_>, Vec<_>) = receipts.into_iter().partition(|receipt| {
      matches!(
        v1::RunInputState::try_from(receipt.state).ok(),
        Some(v1::RunInputState::Pending | v1::RunInputState::CancelRequested)
      )
    });
    let history_truncated = terminal.len() > 100;
    pending.extend(terminal.into_iter().rev().take(100));
    for receipt in &mut pending {
      if receipt.prompt.len() > 4096 {
        let mut end = 4096;
        while !receipt.prompt.is_char_boundary(end) {
          end -= 1;
        }
        receipt.prompt.truncate(end);
        receipt.prompt_truncated = true;
      }
    }
    Ok(v1::RunInputs {
      inputs: pending,
      history_truncated,
    })
  }
}
