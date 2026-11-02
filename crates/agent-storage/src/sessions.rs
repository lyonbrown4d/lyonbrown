use crate::{Store, decode, event_tx, schema::sessions, verify_epoch};
use agent_core::{Session, new_id, now_ms};
use anyhow::{Context, Result};
use sea_orm::{QueryOrder, QuerySelect, Set, prelude::*};
impl Store {
  pub async fn create_session(&self, title: &str, workspace: &str, node: &str) -> Result<Session> {
    anyhow::ensure!(title.len() <= 1024, "session title too long");
    let value = Session {
      id: new_id(),
      title: title.into(),
      workspace: workspace.into(),
      owner_node_id: node.into(),
      owner_epoch: 1,
      created_at_ms: now_ms(),
    };
    sessions::ActiveModel {
      id: Set((&value.id).to_owned()),
      owner_epoch: Set((1_i64).to_owned()),
      record: Set((serde_json::to_string(&value)?).to_owned()),
    }
    .insert(self.connection())
    .await?;
    Ok(value)
  }
  pub async fn session(&self, id: &str) -> Result<Option<Session>> {
    let json: Option<String> = sessions::Entity::find()
      .filter(sessions::Column::Id.eq(id))
      .select_only()
      .column(sessions::Column::Record)
      .into_tuple::<String>()
      .one(self.connection())
      .await?;
    json
      .map(|record| Ok(serde_json::from_str(&record)?))
      .transpose()
  }
  pub async fn sessions(&self) -> Result<Vec<Session>> {
    decode(
      sessions::Entity::find()
        .order_by_desc(sessions::Column::Id)
        .select_only()
        .column(sessions::Column::Record)
        .into_tuple::<String>()
        .all(self.connection())
        .await?,
    )
  }
  pub async fn change_owner(&self, id: &str, expected_epoch: u64, node: &str) -> Result<Session> {
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        verify_epoch(connection, id, expected_epoch).await?;
        let json: String = sessions::Entity::find()
          .filter(sessions::Column::Id.eq(id))
          .select_only()
          .column(sessions::Column::Record)
          .into_tuple::<String>()
          .one(connection)
          .await?
          .ok_or_else(|| anyhow::anyhow!("sessions record missing"))?;
        let mut value: Session = serde_json::from_str(&json)?;
        value.owner_epoch = expected_epoch
          .checked_add(1)
          .context("owner epoch exhausted")?;
        value.owner_node_id = node.into();
        sessions::Entity::update_many()
          .filter(sessions::Column::Id.eq(id))
          .col_expr(
            sessions::Column::OwnerEpoch,
            Expr::value((i64::try_from(value.owner_epoch)?).to_owned()),
          )
          .col_expr(
            sessions::Column::Record,
            Expr::value((serde_json::to_string(&value)?).to_owned()),
          )
          .exec(connection)
          .await
          .map(|result| result.rows_affected)?;
        event_tx(
          connection,
          id,
          "session_owner_changed",
          serde_json::json!({"owner_node_id":node,"owner_epoch":value.owner_epoch}),
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
}
