use crate::{
  Result, StorageError,
  access::unsigned,
  auth::{now_seconds, signed_time},
  schema::gateway_access_tokens as tokens,
};
use gateway_core::{AccessPrincipal, AuditRecord, AuditStatus};
use sea_orm::{ActiveModelTrait, QueryFilter, Set, prelude::*};
use std::collections::BTreeSet;

/// Called only inside the same immediate transaction as the durable audit intent.
pub(crate) async fn reserve(connection: &impl ConnectionTrait, record: &AuditRecord) -> Result<()> {
  let Some(principal) = &record.principal else {
    return if record.reserved_tokens == 0 {
      Ok(())
    } else {
      Err(StorageError::Unauthorized)
    };
  };
  let now = signed_time(now_seconds()?)?;
  let row = tokens::Entity::find_by_id(&principal.key_id)
    .filter(tokens::Column::Revoked.eq(false))
    .filter(tokens::Column::CreatedAt.lte(now))
    .filter(tokens::Column::ExpiresAt.gt(now))
    .one(connection)
    .await?
    .ok_or(StorageError::Unauthorized)?;
  let stored = AccessPrincipal {
    key_id: principal.key_id.clone(),
    project_id: row.project_id.clone(),
    allowed_models: serde_json::from_str::<BTreeSet<String>>(&row.allowed_models)
      .map_err(|_| StorageError::InvalidData)?,
    max_output_tokens: row.max_output_tokens.map(unsigned).transpose()?,
  };
  if &stored != principal
    || (!stored.allowed_models.is_empty() && !stored.allowed_models.contains(&record.model))
  {
    return Err(StorageError::Unauthorized);
  }
  let reserved = signed_time(record.reserved_tokens)?;
  let total = row
    .tokens_charged
    .checked_add(reserved)
    .ok_or(StorageError::BudgetExceeded)?;
  if row.token_budget.is_some_and(|budget| total > budget) {
    return Err(StorageError::BudgetExceeded);
  }
  let mut row: tokens::ActiveModel = row.into();
  row.tokens_charged = Set(total);
  row.update(connection).await?;
  Ok(())
}

/// Unknown outcomes keep the reservation. Actual totals may exceed estimates and remain charged.
pub(crate) async fn settle(connection: &impl ConnectionTrait, record: &AuditRecord) -> Result<()> {
  if matches!(record.status, AuditStatus::Uncertain) {
    return Ok(());
  }
  let (Some(principal), Some(usage)) = (&record.principal, record.usage) else {
    return Ok(());
  };
  let actual = usage
    .input_tokens
    .checked_add(usage.output_tokens)
    .and_then(|total| i64::try_from(total).ok())
    .ok_or(StorageError::InvalidInput("token usage overflow"))?;
  let row = tokens::Entity::find_by_id(&principal.key_id)
    .one(connection)
    .await?
    .ok_or(StorageError::InvalidData)?;
  let charged = row.tokens_charged;
  let reserved = signed_time(record.reserved_tokens)?;
  let total = charged
    .checked_sub(reserved)
    .filter(|total| *total >= 0)
    .and_then(|total| total.checked_add(actual))
    .ok_or(StorageError::InvalidData)?;
  let mut row: tokens::ActiveModel = row.into();
  row.tokens_charged = Set(total);
  row.update(connection).await?;
  Ok(())
}
