//! Durable cancellation intent fences queue reconstruction after a lost result.
use std::collections::BTreeMap;

use agent_core::{
  Event, Run,
  events::{AGENT_EVENT_KIND, AgentEvent, DecodedEvent, QueueStatus},
};
use anyhow::Result;
use diesel::prelude::*;

use crate::{
  Store, event_tx,
  schema::{events, runs, sessions},
  verify_epoch,
};

fn unresolved(
  connection: &mut SqliteConnection,
  session: &str,
) -> Result<BTreeMap<String, String>> {
  let records: Vec<(String, String)> = events::table
    .inner_join(runs::table.on(runs::id.eq(events::run_id)))
    .filter(runs::session_id.eq(session))
    .order((
      runs::created_at_ms.asc(),
      runs::id.asc(),
      events::cursor.asc(),
    ))
    .select((runs::id, events::record))
    .load(connection)?;
  let mut pending = BTreeMap::new();
  for (source, record) in records {
    let event: Event = serde_json::from_str(&record)?;
    anyhow::ensure!(
      event.run_id == source,
      "input event run identity differs from ledger"
    );
    if event.kind != AGENT_EVENT_KIND {
      continue;
    }
    let DecodedEvent::Known(AgentEvent::QueueUpdated {
      entry_id, status, ..
    }) = agent_core::events::decode(&event.kind, &event.payload_json)?
    else {
      continue;
    };
    match status {
      QueueStatus::CancelRequested => {
        pending.insert(entry_id, source);
      }
      QueueStatus::Consumed | QueueStatus::Cancelled => {
        pending.remove(&entry_id);
      }
      QueueStatus::Pending => {}
    }
  }
  Ok(pending)
}

pub(crate) fn ensure_no_input_cancellation(
  connection: &mut SqliteConnection,
  session: &str,
) -> Result<()> {
  anyhow::ensure!(
    unresolved(connection, session)?.is_empty(),
    "session has an unresolved input cancellation; reconcile before starting another run"
  );
  Ok(())
}

fn fence(connection: &mut SqliteConnection, mut run: Run) -> Result<Run> {
  if run.status == "uncertain" {
    return Ok(run);
  }
  let other: i64 = runs::table
    .filter(runs::session_id.eq(&run.session_id))
    .filter(runs::id.ne(&run.id))
    .filter(runs::status.ne_all(["completed", "failed", "cancelled"]))
    .count()
    .get_result(connection)?;
  anyhow::ensure!(
    other == 0,
    "another run owns the session execution fence; input cancellation remains unresolved"
  );
  run.status = "uncertain".into();
  run.error =
    "input cancellation was not durably confirmed; reconcile before further execution".into();
  crate::approvals::close_pending(connection, &run.id)?;
  diesel::update(runs::table.find(&run.id))
    .set((
      runs::status.eq(&run.status),
      runs::record.eq(serde_json::to_string(&run)?),
    ))
    .execute(connection)?;
  event_tx(
    connection,
    &run.id,
    "run_uncertain",
    serde_json::json!({"status":run.status,"result":run.result,"error":run.error}),
  )?;
  Ok(run)
}

impl Store {
  /// Fence even a terminal run when a late queue cancellation cannot commit.
  /// The previously recorded result remains available for reconciliation.
  pub fn fence_run_input_failure(&self, run: &str, epoch: u64) -> Result<Run> {
    self.lock()?.immediate_transaction(|connection| {
      let record: String = runs::table
        .find(run)
        .select(runs::record)
        .first(connection)?;
      let run: Run = serde_json::from_str(&record)?;
      verify_epoch(connection, &run.session_id, epoch)?;
      fence(connection, run)
    })
  }

  /// Restore only host execution fences. Never reconstruct or execute queues.
  /// Returns the number of sessions newly fenced by unresolved input intents.
  pub fn recover_input_cancellations(&self) -> Result<usize> {
    self.lock()?.immediate_transaction(|connection| {
      let ids: Vec<String> = sessions::table.select(sessions::id).load(connection)?;
      let mut restored = 0;
      for session in ids {
        let pending = unresolved(connection, &session)?;
        let Some(source) = pending.values().next() else {
          continue;
        };
        let uncertain: i64 = runs::table
          .filter(runs::session_id.eq(&session))
          .filter(runs::status.eq("uncertain"))
          .count()
          .get_result(connection)?;
        if uncertain != 0 {
          continue;
        }
        let record: String = runs::table
          .find(source)
          .select(runs::record)
          .first(connection)?;
        let run: Run = serde_json::from_str(&record)?;
        anyhow::ensure!(
          run.session_id == session,
          "input cancellation source belongs to another session"
        );
        fence(connection, run)?;
        restored += 1;
      }
      Ok(restored)
    })
  }
}
