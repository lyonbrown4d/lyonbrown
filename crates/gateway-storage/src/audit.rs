use crate::{
  GatewayStorage, Result, StorageError, error::valid_key, schema::gateway_audits as audits,
};
use async_trait::async_trait;
use gateway_core::{AuditError, AuditRecord, AuditSink, AuditStatus};
use sea_orm::{
  ActiveModelTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, prelude::*,
  sea_query::Expr,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AuditPage {
  pub records: Vec<AuditRecord>,
  pub total: u64,
}
type Row = (
  String,
  String,
  i16,
  i64,
  String,
  Option<String>,
  i64,
  Option<String>,
);
impl GatewayStorage {
  pub async fn list_audits(&self, limit: u32, offset: u64) -> Result<AuditPage> {
    if !(1..=200).contains(&limit) {
      return Err(StorageError::InvalidInput("audit page size"));
    }
    let offset = i64::try_from(offset).map_err(|_| StorageError::InvalidInput("audit offset"))?;
    self
      .run(move |connection| {
        Box::pin(async move {
          let total = audits::Entity::find().count(connection).await?;
          let rows = audits::Entity::find()
            .order_by_desc(audits::Column::Sequence)
            .limit(u64::from(limit))
            .offset(offset as u64)
            .all(connection)
            .await?;
          let records = rows
            .into_iter()
            .map(|row| {
              decode((
                row.request_id,
                row.model,
                row.status,
                row.elapsed_ms,
                row.policy_revision,
                row.principal,
                row.reserved_tokens,
                row.usage,
              ))
            })
            .collect::<Result<_>>()?;
          Ok(AuditPage { records, total })
        })
      })
      .await
  }
  pub async fn record_audit(&self, record: AuditRecord) -> Result<()> {
    if !valid_key(&record.request_id)
      || !valid_key(&record.model)
      || !valid_key(&record.policy_revision)
    {
      return Err(StorageError::InvalidInput("audit metadata"));
    }
    let elapsed =
      i64::try_from(record.elapsed_ms).map_err(|_| StorageError::InvalidInput("elapsed time"))?;
    let status = encode(record.status);
    if record.usage.is_some_and(|usage| {
      usage.cached_input_tokens > usage.input_tokens || usage.reasoning_tokens > usage.output_tokens
    }) {
      return Err(StorageError::InvalidInput("token usage subsets"));
    }
    let principal = record
      .principal
      .as_ref()
      .map(serde_json::to_string)
      .transpose()
      .map_err(|_| StorageError::InvalidData)?;
    let usage = record
      .usage
      .as_ref()
      .map(serde_json::to_string)
      .transpose()
      .map_err(|_| StorageError::InvalidData)?;
    let reserved = i64::try_from(record.reserved_tokens)
      .map_err(|_| StorageError::InvalidInput("reserved tokens"))?;
    self
      .write(move |connection| {
        Box::pin(async move {
          if status == 0 {
            if record.usage.is_some() {
              return Err(StorageError::InvalidInput("pending usage"));
            }
            if audits::Entity::find()
              .filter(audits::Column::RequestId.eq(&record.request_id))
              .one(connection)
              .await?
              .is_some()
            {
              return Err(StorageError::Conflict);
            }
            crate::budget::reserve(connection, &record).await?;
            audits::ActiveModel {
              request_id: Set(record.request_id),
              model: Set(record.model),
              status: Set(status),
              elapsed_ms: Set(elapsed),
              policy_revision: Set(record.policy_revision),
              principal: Set(principal),
              reserved_tokens: Set(reserved),
              usage: Set(None),
              ..Default::default()
            }
            .insert(connection)
            .await?;
            return Ok(());
          }
          let previous = audits::Entity::find()
            .filter(audits::Column::RequestId.eq(&record.request_id))
            .one(connection)
            .await?
            .ok_or(StorageError::Conflict)?;
          if previous.model != record.model
            || previous.policy_revision != record.policy_revision
            || previous.principal != principal
            || previous.reserved_tokens != reserved
          {
            return Err(StorageError::Conflict);
          }
          if previous.status == status {
            return if previous.usage == usage {
              Ok(())
            } else {
              Err(StorageError::Conflict)
            };
          }
          if previous.status != 0 {
            return Err(StorageError::Conflict);
          }
          crate::budget::settle(connection, &record).await?;
          let mut previous: audits::ActiveModel = previous.into();
          previous.status = Set(status);
          previous.elapsed_ms = Set(elapsed);
          previous.usage = Set(usage);
          previous.update(connection).await?;
          Ok(())
        })
      })
      .await
  }
}
#[async_trait]
impl AuditSink for GatewayStorage {
  async fn admit(
    &self,
    record: AuditRecord,
  ) -> std::result::Result<(), gateway_core::AdmissionError> {
    self
      .record_audit(record)
      .await
      .map_err(|error| match error {
        StorageError::BudgetExceeded => gateway_core::AdmissionError::BudgetExceeded,
        _ => gateway_core::AdmissionError::Unavailable,
      })
  }
  async fn record(&self, record: AuditRecord) -> std::result::Result<(), AuditError> {
    self.record_audit(record).await.map_err(|_| AuditError)
  }
}
pub(crate) async fn reconcile(connection: &impl ConnectionTrait) -> Result<()> {
  audits::Entity::update_many()
    .col_expr(audits::Column::Status, Expr::value(4_i16))
    .filter(audits::Column::Status.eq(0_i16))
    .exec(connection)
    .await?;
  Ok(())
}
fn encode(status: AuditStatus) -> i16 {
  match status {
    AuditStatus::Pending => 0,
    AuditStatus::Succeeded => 1,
    AuditStatus::Failed => 2,
    AuditStatus::Cancelled => 3,
    AuditStatus::Uncertain => 4,
  }
}
fn decode(row: Row) -> Result<AuditRecord> {
  let status = match row.2 {
    0 => AuditStatus::Pending,
    1 => AuditStatus::Succeeded,
    2 => AuditStatus::Failed,
    3 => AuditStatus::Cancelled,
    4 => AuditStatus::Uncertain,
    _ => return Err(StorageError::InvalidData),
  };
  Ok(AuditRecord {
    request_id: row.0,
    model: row.1,
    status,
    elapsed_ms: u64::try_from(row.3).map_err(|_| StorageError::InvalidData)?,
    policy_revision: row.4,
    principal: row
      .5
      .as_deref()
      .map(serde_json::from_str)
      .transpose()
      .map_err(|_| StorageError::InvalidData)?,
    reserved_tokens: crate::access::unsigned(row.6)?,
    usage: row
      .7
      .as_deref()
      .map(serde_json::from_str)
      .transpose()
      .map_err(|_| StorageError::InvalidData)?,
  })
}
