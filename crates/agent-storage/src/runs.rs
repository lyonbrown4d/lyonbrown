use crate::{Store, decode, event_tx, schema::runs, verify_epoch};
use agent_core::{Run, RunStatus, new_id, now_ms};
use anyhow::Result;
use sea_orm::{QueryOrder, QuerySelect, Set, prelude::*};
impl Store {
  pub async fn accept_run(
    &self,
    session: &str,
    epoch: u64,
    prompt: &str,
    key: &str,
  ) -> Result<(Run, bool)> {
    anyhow::ensure!(
      !key.is_empty() && key.len() <= 256,
      "invalid idempotency key"
    );
    anyhow::ensure!(
      !prompt.trim().is_empty() && prompt.len() <= 1_048_576,
      "invalid prompt length"
    );
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        verify_epoch(connection, session, epoch).await?;
        let existing: Option<String> = runs::Entity::find()
          .filter(runs::Column::SessionId.eq(session))
          .filter(runs::Column::IdempotencyKey.eq(key))
          .select_only()
          .column(runs::Column::Record)
          .into_tuple::<String>()
          .one(connection)
          .await?;
        if let Some(json) = existing {
          let run: Run = serde_json::from_str(&json)?;
          anyhow::ensure!(
            run.prompt == prompt,
            "idempotency key reused with different prompt"
          );
          return Ok((run, false));
        }
        crate::input_fence::ensure_no_input_cancellation(connection, session).await?;
        let active: i64 = runs::Entity::find()
          .filter(runs::Column::SessionId.eq(session))
          .filter(runs::Column::Status.is_in([
            "accepted",
            "running",
            "awaiting_approval",
            "cancel_requested",
            "uncertain",
          ]))
          .count(connection)
          .await
          .map(|value| value as i64)?;
        anyhow::ensure!(
          active == 0,
          "session already has an active run; use explicit steering or follow-up input"
        );
        let run = Run {
          id: new_id(),
          session_id: session.into(),
          status: "accepted".into(),
          prompt: prompt.into(),
          result: String::new(),
          error: String::new(),
          created_at_ms: now_ms(),
        };
        use crate::schema::model_bindings;
        if let Some(record) = model_bindings::Entity::find()
          .filter(model_bindings::Column::ScopeId.eq(session))
          .select_only()
          .column(model_bindings::Column::Record)
          .into_tuple::<String>()
          .one(connection)
          .await?
        {
          model_bindings::ActiveModel {
            scope_id: Set(run.id.to_owned()),
            record: Set((record).to_owned()),
          }
          .insert(connection)
          .await?;
        }
        runs::ActiveModel {
          id: Set(run.id.to_owned()),
          session_id: Set((session).to_owned()),
          idempotency_key: Set((key).to_owned()),
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
          serde_json::json!({"run_id":run.id}),
        )
        .await?;
        Ok((run, true))
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
  pub async fn run(&self, id: &str) -> Result<Option<Run>> {
    let json: Option<String> = runs::Entity::find()
      .filter(runs::Column::Id.eq(id))
      .select_only()
      .column(runs::Column::Record)
      .into_tuple::<String>()
      .one(self.connection())
      .await?;
    json
      .map(|record| Ok(serde_json::from_str(&record)?))
      .transpose()
  }
  pub async fn runs(&self, session: &str) -> Result<Vec<Run>> {
    decode(
      runs::Entity::find()
        .filter(runs::Column::SessionId.eq(session))
        .order_by_asc(runs::Column::CreatedAtMs)
        .order_by_asc(runs::Column::Id)
        .select_only()
        .column(runs::Column::Record)
        .into_tuple::<String>()
        .all(self.connection())
        .await?,
    )
  }
  pub async fn update_run(
    &self,
    id: &str,
    status: RunStatus,
    result: &str,
    error: &str,
  ) -> Result<Run> {
    self.mutate_run(id, None, status, result, error).await
  }
  pub async fn update_run_owned(
    &self,
    id: &str,
    epoch: u64,
    status: RunStatus,
    result: &str,
    error: &str,
  ) -> Result<Run> {
    self
      .mutate_run(id, Some(epoch), status, result, error)
      .await
  }
  async fn mutate_run(
    &self,
    id: &str,
    epoch: Option<u64>,
    status: RunStatus,
    result: &str,
    error: &str,
  ) -> Result<Run> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let json: String = runs::Entity::find()
          .filter(runs::Column::Id.eq(id))
          .select_only()
          .column(runs::Column::Record)
          .into_tuple::<String>()
          .one(connection)
          .await?
          .ok_or_else(|| anyhow::anyhow!("runs record missing"))?;
        let mut run: Run = serde_json::from_str(&json)?;
        if let Some(epoch) = epoch {
          verify_epoch(connection, &run.session_id, epoch).await?;
        }
        anyhow::ensure!(
          !matches!(
            run.status.as_str(),
            "completed" | "failed" | "cancelled" | "uncertain"
          ),
          "run is already terminal"
        );
        // Serialize cancellation against the terminal commit. A successful
        // Runtime completion cannot overwrite a cancellation already accepted
        // by the daemon. Unknown side effects retain their Uncertain outcome.
        let status = if run.status == "cancel_requested" && status == RunStatus::Completed {
          RunStatus::Cancelled
        } else {
          status
        };
        run.status = status.as_str().into();
        run.result = result.into();
        run.error = error.into();
        if status.terminal() {
          crate::approvals::close_pending(connection, id).await?;
        }
        runs::Entity::update_many()
          .filter(runs::Column::Id.eq(id))
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
          id,
          &format!("run_{}", status.as_str()),
          serde_json::json!({"status":run.status,"result":result,"error":error}),
        )
        .await?;
        Ok(run)
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
  pub async fn recover_interrupted(&self) -> Result<Vec<Run>> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let records: Vec<String> = runs::Entity::find()
          .filter(runs::Column::Status.is_not_in([
            "accepted",
            "completed",
            "failed",
            "cancelled",
            "uncertain",
          ]))
          .select_only()
          .column(runs::Column::Record)
          .into_tuple::<String>()
          .all(connection)
          .await?;
        let mut values = Vec::new();
        for record in records {
          let mut run: Run = serde_json::from_str(&record)?;
          run.status = "uncertain".into();
          run.error =
            "daemon stopped before a durable terminal result; reconcile before resuming".into();
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
            serde_json::json!({"error":run.error}),
          )
          .await?;
          values.push(run);
        }
        Ok(values)
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
  pub async fn pending_runs(&self) -> Result<Vec<Run>> {
    decode(
      runs::Entity::find()
        .filter(runs::Column::Status.eq("accepted"))
        .order_by_asc(runs::Column::CreatedAtMs)
        .order_by_asc(runs::Column::Id)
        .select_only()
        .column(runs::Column::Record)
        .into_tuple::<String>()
        .all(self.connection())
        .await?,
    )
  }
}
