use crate::approval_consumption::consume_approval_tx;
use crate::{
  Store, decode, event_tx,
  schema::{approvals, runs},
  verify_epoch,
};
use agent_core::{Approval, Run, new_id, now_ms};
use anyhow::Result;
use sea_orm::{QueryOrder, QuerySelect, Set, prelude::*};

pub(crate) async fn close_pending(connection: &impl ConnectionTrait, run: &str) -> Result<()> {
  let pending: Vec<String> = approvals::Entity::find()
    .filter(approvals::Column::RunId.eq(run))
    .filter(approvals::Column::Status.eq("pending"))
    .select_only()
    .column(approvals::Column::Record)
    .into_tuple::<String>()
    .all(connection)
    .await?;
  for record in pending {
    let mut approval: Approval = serde_json::from_str(&record)?;
    approval.status = "cancelled".into();
    approvals::Entity::update_many()
      .filter(approvals::Column::Id.eq(&approval.id))
      .col_expr(
        approvals::Column::Status,
        Expr::value(approval.status.to_owned()),
      )
      .col_expr(
        approvals::Column::Record,
        Expr::value((serde_json::to_string(&approval)?).to_owned()),
      )
      .exec(connection)
      .await
      .map(|result| result.rows_affected)?;
    event_tx(
      connection,
      run,
      "approval_closed",
      serde_json::json!({"approval_id":approval.id,"status":"cancelled"}),
    )
    .await?;
  }
  Ok(())
}

impl Store {
  pub async fn request_approval(
    &self,
    run_id: &str,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<Approval> {
    self
      .request_approval_inner(run_id, None, tool, arguments)
      .await
  }
  pub async fn request_approval_owned(
    &self,
    run_id: &str,
    epoch: u64,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<Approval> {
    self
      .request_approval_inner(run_id, Some(epoch), tool, arguments)
      .await
  }
  async fn request_approval_inner(
    &self,
    run_id: &str,
    epoch: Option<u64>,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<Approval> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let value = Approval {
          id: new_id(),
          run_id: run_id.into(),
          tool: tool.into(),
          arguments_json: serde_json::to_string(arguments)?,
          status: "pending".into(),
          expires_at_ms: now_ms() + 300_000,
        };
        let json: String = runs::Entity::find()
          .filter(runs::Column::Id.eq(run_id))
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
        anyhow::ensure!(run.status == "running", "run is not executing");
        run.status = "awaiting_approval".into();
        runs::Entity::update_many()
          .filter(runs::Column::Id.eq(run_id))
          .col_expr(runs::Column::Status, Expr::value(run.status.to_owned()))
          .col_expr(
            runs::Column::Record,
            Expr::value((serde_json::to_string(&run)?).to_owned()),
          )
          .exec(connection)
          .await
          .map(|result| result.rows_affected)?;
        approvals::ActiveModel {
          id: Set(value.id.to_owned()),
          run_id: Set((run_id).to_owned()),
          status: Set(value.status.to_owned()),
          record: Set((serde_json::to_string(&value)?).to_owned()),
        }
        .insert(connection)
        .await?;
        event_tx(
          connection,
          run_id,
          "approval_required",
          serde_json::json!({"approval_id":value.id,"tool":tool,"arguments":arguments}),
        )
        .await?;
        Ok(value)
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
  pub async fn approvals(&self, run_id: &str) -> Result<Vec<Approval>> {
    decode(
      approvals::Entity::find()
        .filter(approvals::Column::RunId.eq(run_id))
        .order_by_asc(approvals::Column::Id)
        .select_only()
        .column(approvals::Column::Record)
        .into_tuple::<String>()
        .all(self.connection())
        .await?,
    )
  }
  pub async fn approval(&self, id: &str) -> Result<Option<Approval>> {
    let json: Option<String> = approvals::Entity::find()
      .filter(approvals::Column::Id.eq(id))
      .select_only()
      .column(approvals::Column::Record)
      .into_tuple::<String>()
      .one(self.connection())
      .await?;
    json
      .map(|record| Ok(serde_json::from_str(&record)?))
      .transpose()
  }
  pub async fn decide_approval(&self, id: &str, approved: bool) -> Result<Approval> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let json: String = approvals::Entity::find()
          .filter(approvals::Column::Id.eq(id))
          .select_only()
          .column(approvals::Column::Record)
          .into_tuple::<String>()
          .one(connection)
          .await?
          .ok_or_else(|| anyhow::anyhow!("approvals record missing"))?;
        let mut value: Approval = serde_json::from_str(&json)?;
        anyhow::ensure!(value.status == "pending", "approval already decided");
        anyhow::ensure!(value.expires_at_ms > now_ms(), "approval expired");
        let json: String = runs::Entity::find()
          .filter(runs::Column::Id.eq(&value.run_id))
          .select_only()
          .column(runs::Column::Record)
          .into_tuple::<String>()
          .one(connection)
          .await?
          .ok_or_else(|| anyhow::anyhow!("runs record missing"))?;
        let mut run: Run = serde_json::from_str(&json)?;
        anyhow::ensure!(
          run.status == "awaiting_approval",
          "run no longer awaiting approval"
        );
        run.status = "running".into();
        value.status = if approved { "approved" } else { "denied" }.into();
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
        approvals::Entity::update_many()
          .filter(approvals::Column::Id.eq(id))
          .col_expr(
            approvals::Column::Status,
            Expr::value(value.status.to_owned()),
          )
          .col_expr(
            approvals::Column::Record,
            Expr::value((serde_json::to_string(&value)?).to_owned()),
          )
          .exec(connection)
          .await
          .map(|result| result.rows_affected)?;
        event_tx(
          connection,
          &run.id,
          "approval_decided",
          serde_json::json!({"approval_id":id,"status":value.status}),
        )
        .await?;
        Ok(value)
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
  pub async fn consume_approval(
    &self,
    id: &str,
    run_id: &str,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<bool> {
    self
      .consume_approval_inner(id, run_id, None, tool, arguments)
      .await
  }
  pub async fn consume_approval_owned(
    &self,
    id: &str,
    run_id: &str,
    epoch: u64,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<bool> {
    self
      .consume_approval_inner(id, run_id, Some(epoch), tool, arguments)
      .await
  }
  async fn consume_approval_inner(
    &self,
    id: &str,
    run_id: &str,
    epoch: Option<u64>,
    tool: &str,
    arguments: &serde_json::Value,
  ) -> Result<bool> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> =
        async { consume_approval_tx(connection, id, run_id, epoch, tool, arguments).await }.await;
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
