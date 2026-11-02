use crate::{Store, event_tx, schema::peers};
use agent_core::PeerTrust;
use anyhow::Result;
use sea_orm::{QuerySelect, Set, prelude::*, sea_query::OnConflict};
impl Store {
  pub async fn trust_peer(&self, value: &PeerTrust) -> Result<()> {
    let record = serde_json::to_string(value)?;
    {
      let transaction = self.begin_write().await?;
      let connection = &transaction;
      let result: anyhow::Result<_> = async {
        peers::Entity::insert(peers::ActiveModel {
          node_id: Set((&value.node_id).to_owned()),
          record: Set((&record).to_owned()),
        })
        .on_conflict(
          OnConflict::columns([peers::Column::NodeId])
            .update_columns([peers::Column::Record])
            .to_owned(),
        )
        .exec_without_returning(connection)
        .await?;
        event_tx(
          connection,
          &format!("peer:{}", value.node_id),
          "peer_trust_changed",
          serde_json::json!({"revoked":value.revoked,"capabilities":value.capabilities}),
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
  pub async fn peers(&self) -> Result<Vec<PeerTrust>> {
    let records = peers::Entity::find()
      .select_only()
      .column(peers::Column::Record)
      .into_tuple::<String>()
      .all(self.connection())
      .await?;
    crate::decode(records)
  }
  pub async fn peer(&self, id: &str) -> Result<Option<PeerTrust>> {
    let json: Option<String> = peers::Entity::find()
      .filter(peers::Column::NodeId.eq(id))
      .select_only()
      .column(peers::Column::Record)
      .into_tuple::<String>()
      .one(self.connection())
      .await?;
    json
      .map(|record| Ok(serde_json::from_str(&record)?))
      .transpose()
  }
}
