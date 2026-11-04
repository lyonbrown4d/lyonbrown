use crate::schema::run_recoveries;
use agent_core::{Run, Session, new_id, now_ms};
use anyhow::{Context, Result, ensure};
use sea_orm::{QuerySelect, Set, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
  Store, event_tx,
  schema::{runs, sessions, tasks},
  verify_epoch,
};

async fn unresolved_tasks(connection: &impl ConnectionTrait, session: &str) -> Result<i64> {
  Ok(
    tasks::Entity::find()
      .filter(tasks::Column::SessionId.eq(session))
      .filter(tasks::Column::Status.is_not_in(["completed", "failed", "cancelled", "rejected"]))
      .count(connection)
      .await
      .map(|value| value as i64)?,
  )
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
  Abandon,
  ContinueNewSession,
}

/// A resolution records a human decision; it never changes the original outcome.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunRecovery {
  pub run_id: String,
  pub idempotency_key: String,
  pub requested_owner_epoch: u64,
  pub fenced_owner_epoch: u64,
  pub action: RecoveryAction,
  pub note: String,
  pub prompt: String,
  pub session_id: Option<String>,
  pub replacement_run_id: Option<String>,
  pub created_at_ms: i64,
}

pub struct RecoverRun<'a> {
  pub run_id: &'a str,
  pub owner_epoch: u64,
  pub node_id: &'a str,
  pub action: RecoveryAction,
  pub idempotency_key: &'a str,
  pub note: &'a str,
  pub prompt: &'a str,
}

impl Store {
  pub async fn unresolved_recovery_tasks(&self, session: &str) -> Result<i64> {
    unresolved_tasks(self.connection(), session).await
  }

  pub async fn run_recovery(&self, id: &str) -> Result<Option<RunRecovery>> {
    let record: Option<String> = run_recoveries::Entity::find()
      .filter(run_recoveries::Column::RunId.eq(id))
      .select_only()
      .column(run_recoveries::Column::Record)
      .into_tuple::<String>()
      .one(self.connection())
      .await?;
    record
      .map(|json| Ok(serde_json::from_str(&json)?))
      .transpose()
  }

  /// Persist the explicit decision and any fresh execution intent atomically.
  /// One original run has one resolution. Retries must reproduce every input.
  pub async fn recover_run(&self, input: RecoverRun<'_>) -> Result<(RunRecovery, bool)> {
    ensure!(
      !input.idempotency_key.is_empty() && input.idempotency_key.len() <= 256,
      "invalid idempotency key"
    );
    ensure!(
      !input.note.trim().is_empty() && input.note.len() <= 4096,
      "human verification note is required (maximum 4096 bytes)"
    );
    match input.action {
      RecoveryAction::Abandon => {
        ensure!(input.prompt.is_empty(), "abandon must not include a prompt")
      }
      RecoveryAction::ContinueNewSession => ensure!(
        !input.prompt.trim().is_empty() && input.prompt.len() <= 1_048_576,
        "a fresh prompt is required (maximum 1048576 bytes)"
      ),
    }
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
      let record: String = runs::Entity::find()
        .filter(runs::Column::Id.eq(input.run_id))
        .select_only()
        .column(runs::Column::Record)
        .into_tuple::<String>()
        .one(connection)
        .await?
        .ok_or_else(|| anyhow::anyhow!("runs record missing"))?;
      let original: Run = serde_json::from_str(&record)?;
      let record: String = sessions::Entity::find()
        .filter(sessions::Column::Id.eq(&original.session_id))
        .select_only()
        .column(sessions::Column::Record)
        .into_tuple::<String>()
        .one(connection)
        .await?
        .ok_or_else(|| anyhow::anyhow!("sessions record missing"))?;
      let mut session: Session = serde_json::from_str(&record)?;
      ensure!(
        session.owner_node_id == input.node_id && !session.id.starts_with("worker:"),
        "only the local Master can resolve this run"
      );
      ensure!(
        original.status == "uncertain",
        "only an uncertain run can be resolved"
      );
      let previous: Option<String> = run_recoveries::Entity::find()
        .filter(run_recoveries::Column::RunId.eq(input.run_id))
        .select_only()
        .column(run_recoveries::Column::Record)
        .into_tuple::<String>()
        .one(connection)
        .await?;
      if let Some(record) = previous {
        let previous: RunRecovery = serde_json::from_str(&record)?;
        verify_epoch(
          connection,
          &original.session_id,
          previous.fenced_owner_epoch,
        )
        .await?;
        ensure!(
          previous.requested_owner_epoch == input.owner_epoch
            && previous.idempotency_key == input.idempotency_key
            && previous.action == input.action
            && previous.note == input.note
            && previous.prompt == input.prompt,
          "run already resolved; retry must match the original recovery request"
        );
        return Ok((previous, false));
      }
      verify_epoch(connection, &original.session_id, input.owner_epoch).await?;
      ensure!(
        unresolved_tasks(connection, &original.session_id).await? == 0,
        "reconcile and cancel or accept/reject unresolved Tasks in the original session before Run recovery"
      );
      session.owner_epoch = input
        .owner_epoch
        .checked_add(1)
        .context("owner epoch exhausted")?;
      let mut resolution = RunRecovery {
        run_id: original.id.clone(),
        idempotency_key: input.idempotency_key.into(),
        requested_owner_epoch: input.owner_epoch,
        fenced_owner_epoch: session.owner_epoch,
        action: input.action,
        note: input.note.into(),
        prompt: input.prompt.into(),
        session_id: None,
        replacement_run_id: None,
        created_at_ms: now_ms(),
      };
      if input.action == RecoveryAction::ContinueNewSession {
        let fresh = Session {
          id: new_id(),
          title: format!("Recovery of {}", original.id),
          workspace: session.workspace.clone(),
          owner_node_id: input.node_id.into(),
          owner_epoch: 1,
          created_at_ms: now_ms(),
        };
        let run = Run {
          id: new_id(),
          session_id: fresh.id.clone(),
          status: "accepted".into(),
          prompt: input.prompt.into(),
          result: String::new(),
          error: String::new(),
          created_at_ms: now_ms(),
        };
        sessions::ActiveModel {
          id: Set(fresh.id.to_owned()),
          owner_epoch: Set((1_i64).to_owned()),
          record: Set((serde_json::to_string(&fresh)?).to_owned()),
        }
        .insert(connection)
        .await?;
        runs::ActiveModel {
          id: Set(run.id.to_owned()),
          session_id: Set(fresh.id.to_owned()),
          idempotency_key: Set((input.idempotency_key).to_owned()),
          status: Set(run.status.to_owned()),
          created_at_ms: Set((run.created_at_ms).to_owned()),
          record: Set((serde_json::to_string(&run)?).to_owned()),
        }
        .insert(connection)
        .await?;
        event_tx(
          connection,
          &run.id,
          "run_accepted",
          serde_json::json!({"run_id":run.id,"recovery_of":original.id}),
        )
        .await?;
        crate::interaction::inherit_recovery_mode(connection, &original.id, &fresh.id, &run.id).await?;
        // Runtime snapshots, tool frames and approvals are deliberately not copied.
        resolution.session_id = Some(fresh.id);
        resolution.replacement_run_id = Some(run.id);
      }
      // Revoke late callbacks before accepting the human resolution. The original
      // Run and Runtime snapshot remain unchanged, including uncertain evidence.
      sessions::Entity::update_many()
        .filter(sessions::Column::Id.eq(&session.id))
        .col_expr(
          sessions::Column::OwnerEpoch,
          Expr::value((i64::try_from(session.owner_epoch)?).to_owned()),
        )
        .col_expr(
          sessions::Column::Record,
          Expr::value((serde_json::to_string(&session)?).to_owned()),
        )
        .exec(connection)
        .await
        .map(|result| result.rows_affected)?;
      run_recoveries::ActiveModel {
        run_id: Set(original.id.to_owned()),
        idempotency_key: Set((input.idempotency_key).to_owned()),
        record: Set((serde_json::to_string(&resolution)?).to_owned()),
      }
      .insert(connection)
      .await
      .context("persist recovery decision")?;
      event_tx(
        connection,
        &original.id,
        "run_recovery_recorded",
        serde_json::to_value(&resolution)?,
      )
      .await?;
      Ok((resolution, true))
    }.await;
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
