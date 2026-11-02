use crate::schema::browser_actions;
use crate::{Store, event_tx, verify_epoch};
use anyhow::{Result, ensure};
use sea_orm::{QuerySelect, Set, prelude::*};
use serde_json::{Value, json};

impl Store {
  /// Record before dispatch. Repeated identifiers are rejected even after restart;
  /// a transport failure never authorizes replaying a browser side effect.
  pub async fn begin_browser_action(
    &self,
    session: &str,
    epoch: u64,
    action: &str,
    kind: &str,
    arguments: Value,
  ) -> Result<()> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        begin_browser_action_tx(connection, session, epoch, action, kind, arguments).await
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
  pub async fn finish_browser_action(
    &self,
    action: &str,
    epoch: u64,
    status: &str,
    summary: Value,
  ) -> Result<()> {
    ensure!(
      matches!(status, "completed" | "failed" | "uncertain"),
      "invalid browser outcome"
    );
    let result = serde_json::to_string(&summary)?;
    ensure!(
      result.len() <= 1024 * 1024,
      "browser result summary exceeds limit"
    );
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let (session, actual): (String, i64) = browser_actions::Entity::find()
          .filter(browser_actions::Column::ActionId.eq(action))
          .select_only()
          .column(browser_actions::Column::SessionId)
          .column(browser_actions::Column::OwnerEpoch)
          .into_tuple::<(String, i64)>()
          .one(connection)
          .await?
          .ok_or_else(|| anyhow::anyhow!("browser_actions record missing"))?;
        ensure!(
          u64::try_from(actual)? == epoch,
          "stale browser action epoch"
        );
        verify_epoch(connection, &session, epoch).await?;
        ensure!(
          browser_actions::Entity::update_many()
            .filter(browser_actions::Column::ActionId.eq(action))
            .filter(browser_actions::Column::Status.eq("intent"))
            .col_expr(
              browser_actions::Column::Status,
              Expr::value((status).to_owned())
            )
            .col_expr(
              browser_actions::Column::Result,
              Expr::value((result).to_owned())
            )
            .exec(connection)
            .await
            .map(|result| result.rows_affected)?
            == 1,
          "browser outcome already recorded"
        );
        event_tx(
          connection,
          &format!("browser:{session}"),
          "browser_action_finished",
          json!({"action_id":action,"status":status}),
        )
        .await?;
        Ok(())
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
  pub async fn browser_action_status(&self, action: &str) -> Result<Option<String>> {
    Ok(
      browser_actions::Entity::find()
        .filter(browser_actions::Column::ActionId.eq(action))
        .select_only()
        .column(browser_actions::Column::Status)
        .into_tuple::<String>()
        .one(self.connection())
        .await?,
    )
  }
  pub async fn recover_browser_actions(&self) -> Result<usize> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        let interrupted = browser_actions::Entity::find()
          .filter(browser_actions::Column::Status.eq("intent"))
          .select_only()
          .column(browser_actions::Column::ActionId)
          .column(browser_actions::Column::SessionId)
          .into_tuple::<(String, String)>()
          .all(connection)
          .await?;
        for (id, session) in &interrupted {
          browser_actions::Entity::update_many()
            .filter(browser_actions::Column::ActionId.eq(id))
            .col_expr(
              browser_actions::Column::Status,
              Expr::value(("uncertain").to_owned()),
            )
            .exec(connection)
            .await
            .map(|result| result.rows_affected)?;
          event_tx(
            connection,
            &format!("browser:{session}"),
            "browser_action_finished",
            json!({"action_id":id,"status":"uncertain","reason":"daemon_restarted"}),
          )
          .await?;
        }
        Ok(interrupted.len())
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

/// Shared by authenticated human intent and atomic model approval/intent writes.
pub(crate) async fn begin_browser_action_tx(
  connection: &impl ConnectionTrait,
  session: &str,
  epoch: u64,
  action: &str,
  kind: &str,
  arguments: Value,
) -> Result<()> {
  ensure!(
    !action.is_empty() && action.len() <= 128,
    "invalid browser action identifier"
  );
  let arguments = serde_json::to_string(&arguments)?;
  ensure!(
    arguments.len() <= 1024 * 1024,
    "browser action arguments exceed limit"
  );
  verify_epoch(connection, session, epoch).await?;
  let prior = browser_actions::Entity::find()
    .filter(browser_actions::Column::ActionId.eq(action))
    .select_only()
    .column(browser_actions::Column::ActionId)
    .into_tuple::<String>()
    .one(connection)
    .await?;
  ensure!(
    prior.is_none(),
    "browser action already recorded; inspect its outcome before using a new action identifier"
  );
  browser_actions::ActiveModel {
    action_id: Set((action).to_owned()),
    session_id: Set((session).to_owned()),
    owner_epoch: Set((i64::try_from(epoch)?).to_owned()),
    kind: Set((kind).to_owned()),
    arguments: Set((arguments).to_owned()),
    status: Set(("intent").to_owned()),
    result: Set(("").to_owned()),
  }
  .insert(connection)
  .await?;
  event_tx(
    connection,
    &format!("browser:{session}"),
    "browser_action_intent",
    json!({"action_id":action,"kind":kind}),
  )
  .await?;
  Ok(())
}
