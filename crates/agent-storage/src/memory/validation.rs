use crate::{
  schema::{events, runs},
  verify_epoch,
};
use anyhow::Result;
use memory_core::{MemoryError, MemoryMutation, MemorySource, MemoryState};
use sea_orm::prelude::*;
pub(super) fn validate(mutation: &MemoryMutation) -> Result<()> {
  let item = &mutation.item;
  let scope = match &item.scope {
    memory_core::MemoryScope::User(id)
    | memory_core::MemoryScope::Project(id)
    | memory_core::MemoryScope::Session(id) => id,
  };
  if item.id.is_empty()
    || item.id.len() > 128
    || scope.is_empty()
    || scope.len() > 4096
    || item.content.trim().is_empty()
    || item.content.len() > 65_536
    || item.state == MemoryState::Deleted
    || mutation.expected_revision.checked_add(1) != Some(item.revision)
    || item.updated_at_ms < item.created_at_ms
  {
    return Err(MemoryError::Invalid.into());
  }
  if let MemorySource::Explicit { request_id } = &mutation.source
    && uuid::Uuid::parse_str(request_id).is_err()
  {
    return Err(MemoryError::Invalid.into());
  }
  Ok(())
}
pub(super) async fn validate_source(
  connection: &impl ConnectionTrait,
  mutation: &MemoryMutation,
) -> Result<()> {
  if let MemorySource::Session {
    session_id,
    run_id,
    event_cursor,
    owner_epoch,
  } = &mutation.source
  {
    verify_epoch(connection, session_id, *owner_epoch)
      .await
      .map_err(|error| {
        if crate::is_database_error(&error) {
          error
        } else {
          MemoryError::Forbidden.into()
        }
      })?;
    let run = runs::Entity::find_by_id(run_id)
      .one(connection)
      .await?
      .ok_or(MemoryError::Forbidden)?;
    if run.session_id != *session_id
      || matches!(run.status.as_str(), "uncertain" | "failed" | "cancelled")
      || (mutation.item.state == MemoryState::Active && run.status != "completed")
    {
      return Err(MemoryError::Forbidden.into());
    }
    if let memory_core::MemoryScope::Session(id) = &mutation.item.scope
      && id != session_id
    {
      return Err(MemoryError::Forbidden.into());
    }
    if events::Entity::find_by_id((run_id.clone(), i64::try_from(*event_cursor)?))
      .one(connection)
      .await?
      .is_none()
    {
      return Err(MemoryError::Forbidden.into());
    }
  }
  Ok(())
}
