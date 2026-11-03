use sea_orm::{QueryOrder, QuerySelect, prelude::*};
// Durable cancellation intent fences queue reconstruction after a lost result.
use std::collections::BTreeMap;

use agent_core::{
  Event, Run,
  events::{AGENT_EVENT_KIND, AgentEvent, DecodedEvent, QueueStatus},
};
use anyhow::Result;

use crate::{
  Store, event_tx,
  schema::{events, runs, sessions},
  verify_epoch,
};

async fn unresolved(
  connection: &impl ConnectionTrait,
  session: &str,
) -> Result<BTreeMap<String, String>> {
  let run_rows = runs::Entity::find()
    .filter(runs::Column::SessionId.eq(session))
    .order_by_asc(runs::Column::CreatedAtMs)
    .order_by_asc(runs::Column::Id)
    .all(connection)
    .await?;
  let mut records: Vec<(String, String)> = Vec::new();
  for run in run_rows {
    let event_rows = events::Entity::find()
      .filter(events::Column::RunId.eq(&run.id))
      .order_by_asc(events::Column::Cursor)
      .all(connection)
      .await?;
    records.extend(
      event_rows
        .into_iter()
        .map(|event| (run.id.clone(), event.record)),
    );
  }
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

pub(crate) async fn ensure_no_input_cancellation(
  connection: &impl ConnectionTrait,
  session: &str,
) -> Result<()> {
  anyhow::ensure!(
    unresolved(connection, session).await?.is_empty(),
    "session has an unresolved input cancellation; reconcile before starting another run"
  );
  Ok(())
}

async fn fence(connection: &impl ConnectionTrait, mut run: Run) -> Result<Run> {
  if run.status == "uncertain" {
    return Ok(run);
  }
  let other: i64 = runs::Entity::find()
    .filter(runs::Column::SessionId.eq(&run.session_id))
    .filter(runs::Column::Id.ne(&run.id))
    .filter(runs::Column::Status.is_not_in(["completed", "failed", "cancelled"]))
    .count(connection)
    .await
    .map(|value| value as i64)?;
  anyhow::ensure!(
    other == 0,
    "another run owns the session execution fence; input cancellation remains unresolved"
  );
  run.status = "uncertain".into();
  run.error =
    "input cancellation was not durably confirmed; reconcile before further execution".into();
  crate::approvals::close_pending(connection, &run.id).await?;
  runs::Entity::update_many()
    .filter(runs::Column::Id.eq(&run.id))
    .col_expr(runs::Column::Status, Expr::value(run.status.to_owned()))
    .col_expr(
      runs::Column::Record,
      Expr::value((serde_json::to_string(&run)?).to_owned()),
    )
    .exec(connection)
    .await
    .map(|result| result.rows_affected)?;
  event_tx(
    connection,
    &run.id,
    "run_uncertain",
    serde_json::json!({"status":run.status,"result":run.result,"error":run.error}),
  )
  .await?;
  Ok(run)
}

impl Store {
  /// Fence even a terminal run when a late queue cancellation cannot commit.
  /// The previously recorded result remains available for reconciliation.
  pub async fn fence_run_input_failure(&self, run: &str, epoch: u64) -> Result<Run> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let record: String = runs::Entity::find()
          .filter(runs::Column::Id.eq(run))
          .select_only()
          .column(runs::Column::Record)
          .into_tuple::<String>()
          .one(connection)
          .await?
          .ok_or_else(|| anyhow::anyhow!("runs record missing"))?;
        let run: Run = serde_json::from_str(&record)?;
        verify_epoch(connection, &run.session_id, epoch).await?;
        fence(connection, run).await
      }
      .await;
      match result {
        Ok(value) => {
          transaction.commit().await?;
          Ok::<_, anyhow::Error>(value)
        }
        Err(error) => {
          transaction.rollback().await?;
          Err(error)
        }
      }
    }
  }

  /// Restore only host execution fences. Never reconstruct or execute queues.
  /// Returns the number of sessions newly fenced by unresolved input intents.
  pub async fn recover_input_cancellations(&self) -> Result<usize> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let ids: Vec<String> = sessions::Entity::find()
          .select_only()
          .column(sessions::Column::Id)
          .into_tuple::<String>()
          .all(connection)
          .await?;
        let mut restored = 0;
        for session in ids {
          let pending = unresolved(connection, &session).await?;
          let Some(source) = pending.values().next() else {
            continue;
          };
          let uncertain: i64 = runs::Entity::find()
            .filter(runs::Column::SessionId.eq(&session))
            .filter(runs::Column::Status.eq("uncertain"))
            .count(connection)
            .await
            .map(|value| value as i64)?;
          if uncertain != 0 {
            continue;
          }
          let record: String = runs::Entity::find()
            .filter(runs::Column::Id.eq(source))
            .select_only()
            .column(runs::Column::Record)
            .into_tuple::<String>()
            .one(connection)
            .await?
            .ok_or_else(|| anyhow::anyhow!("runs record missing"))?;
          let run: Run = serde_json::from_str(&record)?;
          anyhow::ensure!(
            run.session_id == session,
            "input cancellation source belongs to another session"
          );
          fence(connection, run).await?;
          restored += 1;
        }
        Ok(restored)
      }
      .await;
      match result {
        Ok(value) => {
          transaction.commit().await?;
          Ok::<_, anyhow::Error>(value)
        }
        Err(error) => {
          transaction.rollback().await?;
          Err(error)
        }
      }
    }
  }
}
