use crate::{Store, schema::runs, verify_epoch};
use agent_core::Run;
use agent_media::ImageAttachment;
use anyhow::{Result, ensure};
use sea_orm::prelude::*;
impl Store {
  /// Read-only acknowledgement lookup precedes capability changes and capacity
  /// admission. It never dispatches or recreates an already accepted run.
  pub async fn accepted_run_with_media(
    &self,
    session: &str,
    epoch: u64,
    prompt: &str,
    key: &str,
    images: &[ImageAttachment],
  ) -> Result<Option<Run>> {
    ensure!(
      !key.is_empty() && key.len() <= 256,
      "invalid idempotency key"
    );
    agent_media::validate_images(prompt, images)?;
    verify_epoch(self.connection(), session, epoch).await?;
    let row = runs::Entity::find()
      .filter(runs::Column::SessionId.eq(session))
      .filter(runs::Column::IdempotencyKey.eq(key))
      .one(self.connection())
      .await?;
    let Some(row) = row else {
      return Ok(None);
    };
    let run: Run = serde_json::from_str(&row.record)?;
    ensure!(
      run.prompt == prompt,
      "idempotency key reused with different prompt"
    );
    let existing = super::load(self.connection(), &run.id).await?;
    ensure!(
      existing.as_slice() == images,
      "idempotency key reused with different attachments"
    );
    Ok(Some(run))
  }
}
