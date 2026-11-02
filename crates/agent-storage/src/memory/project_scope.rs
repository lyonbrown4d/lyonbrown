use super::{digest, entities::projects};
use crate::{Store, event_tx};
use anyhow::Result;
use memory_core::{MemoryError, MemoryScope};
use sea_orm::{Set, prelude::*};
impl Store {
  pub(super) async fn memory_project_scope(
    &self,
    owner: &str,
    canonical_workspace: &str,
  ) -> Result<MemoryScope> {
    if owner.trim().is_empty()
      || owner.len() > 256
      || canonical_workspace.is_empty()
      || canonical_workspace.len() > 16_384
      || !std::path::Path::new(canonical_workspace).is_absolute()
    {
      return Err(MemoryError::Invalid.into());
    }
    let workspace_hash = digest(canonical_workspace.as_bytes());
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      if let Some(row) = projects::Entity::find()
        .filter(projects::Column::OwnerId.eq(owner))
        .filter(projects::Column::WorkspaceHash.eq(&workspace_hash))
        .one(&transaction)
        .await?
      {
        // Retain and compare the original canonical path; hash equality is not identity.
        if row.workspace != canonical_workspace {
          return Err(MemoryError::Conflict.into());
        }
        return Ok(MemoryScope::Project(row.id));
      }
      let id = agent_core::new_id();
      projects::ActiveModel {
        id: Set(id.clone()),
        owner_id: Set(owner.into()),
        workspace_hash: Set(workspace_hash),
        workspace: Set(canonical_workspace.into()),
      }
      .insert(&transaction)
      .await?;
      event_tx(
        &transaction,
        &format!("memory_project:{id}"),
        "memory_project_registered",
        serde_json::json!({"project_id":id}),
      )
      .await?;
      Ok(MemoryScope::Project(id))
    }
    .await;
    match result {
      Ok(value) => {
        transaction.commit().await?;
        Ok(value)
      }
      Err(error) => {
        transaction.rollback().await?;
        Err(error)
      }
    }
  }
}
