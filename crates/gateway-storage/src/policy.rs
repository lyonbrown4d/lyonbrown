use crate::{
  GatewayStorage, Result, StorageError,
  error::valid_key,
  schema::{gateway_policy_head as head, gateway_policy_revisions as revisions},
};
use decision_core::{Policy, PolicySnapshot};
use sea_orm::{ActiveModelTrait, Set, prelude::*, sea_query::OnConflict};
impl GatewayStorage {
  /// Compare-and-swap the complete validated policy. None requires that no head exists.
  pub async fn save_policy(&self, policy: Policy, expected_revision: Option<String>) -> Result<()> {
    PolicySnapshot::new(policy.clone())
      .map_err(|_| StorageError::InvalidInput("decision policy"))?;
    if !valid_key(&policy.revision) {
      return Err(StorageError::InvalidInput("policy revision"));
    }
    let document = serde_json::to_string(&policy).map_err(|_| StorageError::InvalidData)?;
    if document.len() > 1_048_576 {
      return Err(StorageError::InvalidInput("policy size"));
    }
    self
      .write(move |connection| {
        Box::pin(async move {
          let current = head::Entity::find_by_id(1)
            .one(connection)
            .await?
            .map(|row| row.revision);
          if current != expected_revision {
            return Err(StorageError::Conflict);
          }
          let existing = revisions::Entity::find_by_id(&policy.revision)
            .one(connection)
            .await?;
          if existing
            .as_ref()
            .is_some_and(|previous| previous.document != document)
          {
            return Err(StorageError::Conflict);
          }
          if existing.is_none() {
            revisions::ActiveModel {
              revision: Set(policy.revision.clone()),
              document: Set(document),
            }
            .insert(connection)
            .await?;
          }
          head::Entity::insert(head::ActiveModel {
            id: Set(1),
            revision: Set(policy.revision),
          })
          .on_conflict(
            OnConflict::column(head::Column::Id)
              .update_column(head::Column::Revision)
              .to_owned(),
          )
          .exec(connection)
          .await?;
          Ok(())
        })
      })
      .await
  }
  pub async fn load_policy(&self) -> Result<Option<Policy>> {
    self
      .run(|connection| {
        Box::pin(async move {
          let Some(head) = head::Entity::find_by_id(1).one(connection).await? else {
            return Ok(None);
          };
          let row = revisions::Entity::find_by_id(&head.revision)
            .one(connection)
            .await?
            .ok_or(StorageError::InvalidData)?;
          let policy: Policy =
            serde_json::from_str(&row.document).map_err(|_| StorageError::InvalidData)?;
          PolicySnapshot::new(policy.clone()).map_err(|_| StorageError::InvalidData)?;
          if !valid_key(&policy.revision) || policy.revision != head.revision {
            return Err(StorageError::InvalidData);
          }
          Ok(Some(policy))
        })
      })
      .await
  }
}
