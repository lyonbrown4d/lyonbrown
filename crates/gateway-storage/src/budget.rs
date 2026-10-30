use crate::{
  Result, StorageError,
  access::unsigned,
  auth::{now_seconds, signed_time},
  schema::gateway_access_tokens as tokens,
};
use diesel::prelude::*;
use gateway_core::{AccessPrincipal, AuditRecord, AuditStatus};
use std::collections::BTreeSet;

/// Called only inside the same immediate transaction as the durable audit intent.
pub(crate) fn reserve(connection: &mut SqliteConnection, record: &AuditRecord) -> Result<()> {
  let Some(principal) = &record.principal else {
    return if record.reserved_tokens == 0 {
      Ok(())
    } else {
      Err(StorageError::Unauthorized)
    };
  };
  let now = signed_time(now_seconds()?)?;
  let row = tokens::table
    .find(&principal.key_id)
    .filter(tokens::revoked.eq(false))
    .filter(tokens::created_at.le(now))
    .filter(tokens::expires_at.gt(now))
    .select((
      tokens::project_id,
      tokens::allowed_models,
      tokens::max_output_tokens,
      tokens::token_budget,
      tokens::tokens_charged,
    ))
    .first::<(String, String, Option<i64>, Option<i64>, i64)>(connection)
    .optional()?
    .ok_or(StorageError::Unauthorized)?;
  let stored = AccessPrincipal {
    key_id: principal.key_id.clone(),
    project_id: row.0,
    allowed_models: serde_json::from_str::<BTreeSet<String>>(&row.1)
      .map_err(|_| StorageError::InvalidData)?,
    max_output_tokens: row.2.map(unsigned).transpose()?,
  };
  if &stored != principal
    || (!stored.allowed_models.is_empty() && !stored.allowed_models.contains(&record.model))
  {
    return Err(StorageError::Unauthorized);
  }
  let reserved = signed_time(record.reserved_tokens)?;
  let total = row
    .4
    .checked_add(reserved)
    .ok_or(StorageError::BudgetExceeded)?;
  if row.3.is_some_and(|budget| total > budget) {
    return Err(StorageError::BudgetExceeded);
  }
  diesel::update(tokens::table.find(&principal.key_id))
    .set(tokens::tokens_charged.eq(total))
    .execute(connection)?;
  Ok(())
}

/// Unknown outcomes keep the reservation. Actual totals may exceed estimates and remain charged.
pub(crate) fn settle(connection: &mut SqliteConnection, record: &AuditRecord) -> Result<()> {
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
  let charged = tokens::table
    .find(&principal.key_id)
    .select(tokens::tokens_charged)
    .first::<i64>(connection)?;
  let reserved = signed_time(record.reserved_tokens)?;
  let total = charged
    .checked_sub(reserved)
    .filter(|total| *total >= 0)
    .and_then(|total| total.checked_add(actual))
    .ok_or(StorageError::InvalidData)?;
  diesel::update(tokens::table.find(&principal.key_id))
    .set(tokens::tokens_charged.eq(total))
    .execute(connection)?;
  Ok(())
}
