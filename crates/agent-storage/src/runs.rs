use crate::{Store, decode, event_tx, schema::runs, verify_epoch};
use agent_core::{Run, RunStatus};
use anyhow::Result;
use sea_orm::{QueryOrder, QuerySelect, prelude::*};
impl Store {
  pub async fn accept_run(
    &self,
    session: &str,
    epoch: u64,
    prompt: &str,
    key: &str,
  ) -> Result<(Run, bool)> {
    self
      .accept_run_with_media(session, epoch, prompt, key, &[])
      .await
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
