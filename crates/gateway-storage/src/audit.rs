use crate::{
  GatewayStorage, Result, StorageError, error::valid_key, schema::gateway_audits as audits,
};
use async_trait::async_trait;
use diesel::prelude::*;
use gateway_core::{AuditError, AuditRecord, AuditSink, AuditStatus};
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
        connection.transaction(|connection| {
          let total = audits::table.count().get_result::<i64>(connection)?;
          let rows = audits::table
            .order(audits::sequence.desc())
            .limit(i64::from(limit))
            .offset(offset)
            .select((
              audits::request_id,
              audits::model,
              audits::status,
              audits::elapsed_ms,
              audits::policy_revision,
              audits::principal,
              audits::reserved_tokens,
              audits::usage,
            ))
            .load::<Row>(connection)?;
          let records = rows.into_iter().map(decode).collect::<Result<_>>()?;
          Ok(AuditPage {
            records,
            total: u64::try_from(total).map_err(|_| StorageError::InvalidData)?,
          })
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
      .run(move |connection| {
        connection.immediate_transaction(|connection| {
          if status == 0 {
            if record.usage.is_some() {
              return Err(StorageError::InvalidInput("pending usage"));
            }
            crate::budget::reserve(connection, &record)?;
            let changed = diesel::insert_into(audits::table)
              .values((
                audits::request_id.eq(&record.request_id),
                audits::model.eq(&record.model),
                audits::status.eq(status),
                audits::elapsed_ms.eq(elapsed),
                audits::policy_revision.eq(&record.policy_revision),
                audits::principal.eq(&principal),
                audits::reserved_tokens.eq(reserved),
              ))
              .on_conflict_do_nothing()
              .execute(connection)?;
            if changed != 1 {
              return Err(StorageError::Conflict);
            }
            return Ok(());
          }
          let previous = audits::table
            .filter(audits::request_id.eq(&record.request_id))
            .select((
              audits::model,
              audits::status,
              audits::policy_revision,
              audits::principal,
              audits::reserved_tokens,
              audits::usage,
            ))
            .first::<(String, i16, String, Option<String>, i64, Option<String>)>(connection)
            .optional()?;
          let Some((
            model,
            previous_status,
            revision,
            previous_principal,
            previous_reserved,
            previous_usage,
          )) = previous
          else {
            return Err(StorageError::Conflict);
          };
          if model != record.model
            || revision != record.policy_revision
            || principal != previous_principal
            || reserved != previous_reserved
          {
            return Err(StorageError::Conflict);
          }
          if previous_status == status {
            return if usage == previous_usage {
              Ok(())
            } else {
              Err(StorageError::Conflict)
            };
          }
          if previous_status != 0 {
            return Err(StorageError::Conflict);
          }
          crate::budget::settle(connection, &record)?;
          diesel::update(
            audits::table
              .filter(audits::request_id.eq(&record.request_id))
              .filter(audits::status.eq(0)),
          )
          .set((
            audits::status.eq(status),
            audits::elapsed_ms.eq(elapsed),
            audits::usage.eq(usage),
          ))
          .execute(connection)?;
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
pub(crate) fn reconcile(connection: &mut SqliteConnection) -> Result<()> {
  diesel::update(audits::table.filter(audits::status.eq(0)))
    .set(audits::status.eq(4))
    .execute(connection)?;
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
