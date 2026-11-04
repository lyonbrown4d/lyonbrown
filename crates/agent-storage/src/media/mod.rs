//! User attachments are durable private input, separate from Run and event records.
mod accept;
mod entities;
mod lookup;
use crate::Store;
use agent_media::ImageAttachment;
use anyhow::Result;
use sea_orm::{QueryOrder, prelude::*};

pub(super) async fn load(
  connection: &impl ConnectionTrait,
  run: &str,
) -> Result<Vec<ImageAttachment>> {
  let rows = entities::Entity::find()
    .filter(entities::Column::RunId.eq(run))
    .order_by_asc(entities::Column::Ordinal)
    .all(connection)
    .await?;
  Ok(
    rows
      .into_iter()
      .map(|row| ImageAttachment {
        mime_type: row.mime_type,
        data: row.data,
        sha256: row.sha256,
      })
      .collect(),
  )
}
impl Store {
  pub async fn run_media(&self, run: &str) -> Result<Vec<ImageAttachment>> {
    load(self.connection(), run).await
  }
}
