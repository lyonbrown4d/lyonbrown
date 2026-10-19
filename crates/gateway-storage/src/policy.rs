use crate::{
  GatewayStorage, Result, StorageError,
  error::valid_key,
  schema::{gateway_policy_head as head, gateway_policy_revisions as revisions},
};
use decision_core::{Policy, PolicySnapshot};
use diesel::prelude::*;
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
      .run(move |connection| {
        connection.immediate_transaction(|connection| {
          let current = head::table
            .find(1)
            .select(head::revision)
            .first::<String>(connection)
            .optional()?;
          if current != expected_revision {
            return Err(StorageError::Conflict);
          }
          let existing = revisions::table
            .find(&policy.revision)
            .select(revisions::document)
            .first::<String>(connection)
            .optional()?;
          if existing
            .as_ref()
            .is_some_and(|previous| previous != &document)
          {
            return Err(StorageError::Conflict);
          }
          if existing.is_none() {
            diesel::insert_into(revisions::table)
              .values((
                revisions::revision.eq(&policy.revision),
                revisions::document.eq(document),
              ))
              .execute(connection)?;
          }
          diesel::insert_into(head::table)
            .values((head::id.eq(1), head::revision.eq(&policy.revision)))
            .on_conflict(head::id)
            .do_update()
            .set(head::revision.eq(&policy.revision))
            .execute(connection)?;
          Ok(())
        })
      })
      .await
  }
  pub async fn load_policy(&self) -> Result<Option<Policy>> {
    self
      .run(|connection| {
        let revision = head::table
          .find(1)
          .select(head::revision)
          .first::<String>(connection)
          .optional()?;
        let Some(revision) = revision else {
          return Ok(None);
        };
        let document = revisions::table
          .find(&revision)
          .select(revisions::document)
          .first::<String>(connection)?;
        let policy: Policy =
          serde_json::from_str(&document).map_err(|_| StorageError::InvalidData)?;
        PolicySnapshot::new(policy.clone()).map_err(|_| StorageError::InvalidData)?;
        if !valid_key(&policy.revision) || policy.revision != revision {
          return Err(StorageError::InvalidData);
        }
        Ok(Some(policy))
      })
      .await
  }
}
