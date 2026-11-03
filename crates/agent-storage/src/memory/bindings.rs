use super::{
  digest,
  entities::{bindings, items},
};
use crate::{Store, event_tx, schema::runs, verify_epoch};
use anyhow::Result;
use memory_core::{MemoryError, MemoryItem, MemoryScope, MemoryState};
use sea_orm::{Set, prelude::*};
impl Store {
  pub(super) async fn memory_bind_run(
    &self,
    run: &str,
    session: &str,
    epoch: u64,
    memories: &[MemoryItem],
  ) -> Result<()> {
    if memories.len() > 100 {
      return Err(MemoryError::Invalid.into());
    }
    let transaction = self.begin_write().await?;
    let result:Result<_>=async {
      verify_epoch(&transaction,session,epoch).await.map_err(|error| if crate::is_database_error(&error) { error } else { MemoryError::Forbidden.into() })?;
      let run_row=runs::Entity::find_by_id(run).one(&transaction).await?.ok_or(MemoryError::Forbidden)?;
      if run_row.session_id!=session || !matches!(run_row.status.as_str(),"accepted"|"running") {return Err(MemoryError::Forbidden.into());}
      for memory in memories {
        let row=items::Entity::find_by_id(&memory.id).one(&transaction).await?.ok_or(MemoryError::NotFound)?;
        let current:MemoryItem=serde_json::from_str(&row.record)?;
        if current.state!=MemoryState::Active || current.revision!=memory.revision || current!=*memory || current.expires_at_ms.is_some_and(|expiry|expiry<=agent_core::now_ms()) {return Err(MemoryError::Conflict.into());}
        if let MemoryScope::Session(id)=&current.scope && id!=session {return Err(MemoryError::Forbidden.into());}
        let content_hash=digest(current.content.as_bytes());
        if let Some(existing)=bindings::Entity::find_by_id((run.to_owned(),current.id.clone())).one(&transaction).await? {
          if u64::try_from(existing.revision)?!=current.revision || existing.content_hash!=content_hash {return Err(MemoryError::Conflict.into());}
          continue;
        }
        bindings::ActiveModel {run_id:Set(run.into()),memory_id:Set(current.id.clone()),revision:Set(i64::try_from(current.revision)?),content_hash:Set(content_hash.clone())}.insert(&transaction).await?;
        event_tx(&transaction,run,"memory_bound",serde_json::json!({"memory_id":current.id,"revision":current.revision,"content_hash":content_hash})).await?;
      }
      Ok(())
    }.await;
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
