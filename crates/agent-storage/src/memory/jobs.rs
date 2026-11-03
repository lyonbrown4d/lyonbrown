use super::{entities::jobs, job_state};
use crate::{Store, event_tx};
use anyhow::Result;
use memory_core::{MemoryError, MemoryItem, MemoryJob, MemoryJobKind, MemoryJobState, MemoryState};
use sea_orm::{Condition, QueryOrder, QuerySelect, Set, prelude::*};

pub(super) async fn enqueue(connection: &impl ConnectionTrait, item: &MemoryItem) -> Result<()> {
  enqueue_rebuild(connection, item, None, None).await
}
pub(super) async fn enqueue_rebuild(
  connection: &impl ConnectionTrait,
  item: &MemoryItem,
  rebuild_id: Option<&str>,
  embedding: Option<&memory_core::EmbeddingDescriptor>,
) -> Result<()> {
  jobs::ActiveModel {
    id: Set(agent_core::new_id()),
    memory_id: Set(item.id.clone()),
    revision: Set(i64::try_from(item.revision)?),
    kind: Set(
      if item.state == MemoryState::Active {
        "upsert"
      } else {
        "delete"
      }
      .into(),
    ),
    state: Set("pending".into()),
    attempts: Set(0),
    error_category: Set(None),
    rebuild_id: Set(rebuild_id.map(str::to_owned)),
    descriptor_key: Set(embedding.map(super::rebuilds::descriptor_key).transpose()?),
  }
  .insert(connection)
  .await?;
  Ok(())
}
fn decode(row: jobs::Model) -> Result<MemoryJob> {
  Ok(MemoryJob {
    id: row.id,
    memory_id: row.memory_id,
    revision: u64::try_from(row.revision)?,
    kind: match row.kind.as_str() {
      "upsert" => MemoryJobKind::Upsert,
      "delete" => MemoryJobKind::Delete,
      _ => return Err(MemoryError::Storage.into()),
    },
    state: match row.state.as_str() {
      "pending" => MemoryJobState::Pending,
      "running" => MemoryJobState::Running,
      "completed" => MemoryJobState::Completed,
      "failed" => MemoryJobState::Failed,
      "uncertain" => MemoryJobState::Uncertain,
      _ => return Err(MemoryError::Storage.into()),
    },
    attempts: u32::try_from(row.attempts)?,
    error_category: row.error_category,
  })
}
impl Store {
  pub(super) async fn memory_jobs(&self, id: &str) -> Result<Vec<MemoryJob>> {
    jobs::Entity::find()
      .filter(jobs::Column::MemoryId.eq(id))
      .order_by_desc(jobs::Column::Id)
      .limit(100)
      .all(self.connection())
      .await?
      .into_iter()
      .map(decode)
      .collect()
  }
  pub(super) async fn memory_claim_job(
    &self,
    embedding: Option<&memory_core::EmbeddingDescriptor>,
  ) -> Result<Option<MemoryJob>> {
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let mut query = jobs::Entity::find().filter(jobs::Column::State.eq("pending"));
      if let Some(embedding) = embedding {
        query = query.filter(
          Condition::any()
            .add(jobs::Column::DescriptorKey.is_null())
            .add(jobs::Column::DescriptorKey.eq(super::rebuilds::descriptor_key(embedding)?)),
        );
      }
      let Some(mut row) = query
        .order_by_asc(jobs::Column::Id)
        .one(&transaction)
        .await?
      else {
        return Ok(None);
      };
      row.attempts = row.attempts.checked_add(1).ok_or(MemoryError::Storage)?;
      row.state = "running".into();
      row.error_category = None;
      let changed = jobs::Entity::update_many()
        .filter(jobs::Column::Id.eq(&row.id))
        .filter(jobs::Column::State.eq("pending"))
        .col_expr(jobs::Column::State, Expr::value("running"))
        .col_expr(jobs::Column::Attempts, Expr::value(row.attempts))
        .col_expr(
          jobs::Column::ErrorCategory,
          Expr::value(Option::<String>::None),
        )
        .exec(&transaction)
        .await?;
      if changed.rows_affected != 1 {
        return Err(MemoryError::Conflict.into());
      }
      event_tx(
        &transaction,
        &format!("memory:{}", row.memory_id),
        "memory_index_job_claimed",
        serde_json::json!({"job_id":row.id,"revision":row.revision,"attempts":row.attempts}),
      )
      .await?;
      Ok(Some(decode(row)?))
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
  pub(super) async fn memory_finish_job(
    &self,
    id: &str,
    expected_attempt: u32,
    state: MemoryJobState,
    error_category: Option<&str>,
  ) -> Result<()> {
    if !matches!(
      state,
      MemoryJobState::Completed | MemoryJobState::Failed | MemoryJobState::Uncertain
    ) || error_category.is_some_and(|value| {
      value.is_empty()
        || value.len() > 64
        || !value
          .bytes()
          .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'_')
    }) {
      return Err(MemoryError::Invalid.into());
    }
    let transaction = self.begin_write().await?;
    let result:Result<_>=async {
      let row=jobs::Entity::find_by_id(id).one(&transaction).await?.ok_or(MemoryError::NotFound)?;
      if row.state!="running" || row.attempts!=i64::from(expected_attempt) {return Err(MemoryError::Conflict.into());}
      jobs::Entity::update_many().filter(jobs::Column::Id.eq(id)).filter(jobs::Column::State.eq("running")).filter(jobs::Column::Attempts.eq(i64::from(expected_attempt)))
        .col_expr(jobs::Column::State,Expr::value(job_state(state))).col_expr(jobs::Column::ErrorCategory,Expr::value(error_category.map(str::to_owned))).exec(&transaction).await?;
      event_tx(&transaction,&format!("memory:{}",row.memory_id),"memory_index_job_finished",serde_json::json!({"job_id":id,"revision":row.revision,"state":state,"error_category":error_category})).await?;
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
  pub(super) async fn memory_retry_job(
    &self,
    id: &str,
    embedding: Option<&memory_core::EmbeddingDescriptor>,
    check_descriptor: bool,
  ) -> Result<()> {
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let row = jobs::Entity::find_by_id(id)
        .one(&transaction)
        .await?
        .ok_or(MemoryError::NotFound)?;
      if check_descriptor && let Some(target) = &row.descriptor_key {
        let actual = embedding.map(super::rebuilds::descriptor_key).transpose()?;
        if actual.as_ref() != Some(target) {
          return Err(MemoryError::EmbeddingMismatch.into());
        }
      }
      if row.state == "pending" {
        return Ok(());
      }
      if !matches!(row.state.as_str(), "failed" | "uncertain") {
        return Err(MemoryError::Conflict.into());
      }
      jobs::Entity::update_many()
        .filter(jobs::Column::Id.eq(id))
        .filter(jobs::Column::State.eq(&row.state))
        .col_expr(jobs::Column::State, Expr::value("pending"))
        .col_expr(
          jobs::Column::ErrorCategory,
          Expr::value(Option::<String>::None),
        )
        .exec(&transaction)
        .await?;
      event_tx(
        &transaction,
        &format!("memory:{}", row.memory_id),
        "memory_index_job_retried",
        serde_json::json!({"job_id":id,"revision":row.revision,"attempts":row.attempts}),
      )
      .await?;
      Ok(())
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
  pub(super) async fn memory_recover_jobs(&self) -> Result<u64> {
    let transaction = self.begin_write().await?;
    let result: Result<_> = async {
      let rows = jobs::Entity::find()
        .filter(jobs::Column::State.eq("running"))
        .all(&transaction)
        .await?;
      for row in &rows {
        jobs::Entity::update_many()
          .filter(jobs::Column::Id.eq(&row.id))
          .filter(jobs::Column::State.eq("running"))
          .col_expr(jobs::Column::State, Expr::value("uncertain"))
          .col_expr(
            jobs::Column::ErrorCategory,
            Expr::value("worker_interrupted"),
          )
          .exec(&transaction)
          .await?;
        event_tx(
          &transaction,
          &format!("memory:{}", row.memory_id),
          "memory_index_job_recovered",
          serde_json::json!({"job_id":row.id,"revision":row.revision,"state":"uncertain"}),
        )
        .await?;
      }
      Ok(u64::try_from(rows.len())?)
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
