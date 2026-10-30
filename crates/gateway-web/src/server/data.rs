use super::{auth, errors::WebError};
use crate::dto::*;
use axum::http::StatusCode;
use decision_core::ModelTier;
use gateway_core::AuditStatus;
use leptos::prelude::*;
pub(crate) async fn models() -> Result<ModelsView, ServerFnError> {
  let (state, headers) = auth::context().map_err(WebError::server_fn)?;
  let (_, csrf) = auth::authenticated(&state, &headers)
    .await
    .map_err(WebError::server_fn)?;
  Ok(ModelsView {
    csrf,
    ready: state.gateway.is_ready(),
    models: state
      .gateway
      .models()
      .into_iter()
      .map(|model| ModelRowDto {
        id: model.id,
        owned_by: model.owned_by.into(),
      })
      .collect(),
  })
}
pub(crate) async fn policy() -> Result<PolicyView, ServerFnError> {
  let (state, headers) = auth::context().map_err(WebError::server_fn)?;
  let (_, csrf) = auth::authenticated(&state, &headers)
    .await
    .map_err(WebError::server_fn)?;
  let policy = state.engine.snapshot().policy().clone();
  let document =
    serde_json::to_string_pretty(&policy).map_err(|_| WebError::unavailable().server_fn())?;
  Ok(PolicyView {
    csrf,
    document,
    policy_revision: policy.revision,
    domains: policy
      .domains
      .into_iter()
      .map(|rule| DomainRowDto {
        label: rule.label,
        tier: tier(rule.tier).into(),
        terms: rule.terms,
      })
      .collect(),
  })
}
pub(crate) async fn requests(offset: u64, limit: u32) -> Result<RequestsView, ServerFnError> {
  let (state, headers) = auth::context().map_err(WebError::server_fn)?;
  let (_, csrf) = auth::authenticated(&state, &headers)
    .await
    .map_err(WebError::server_fn)?;
  if !(1..=200).contains(&limit) || offset > i64::MAX as u64 {
    return Err(
      WebError {
        status: StatusCode::BAD_REQUEST,
        message: "分页参数无效。",
      }
      .server_fn(),
    );
  }
  let page = state
    .store
    .list_audits(limit, offset)
    .await
    .map_err(|_| WebError::unavailable().server_fn())?;
  Ok(RequestsView {
    csrf,
    offset,
    limit,
    total: page.total,
    records: page
      .records
      .into_iter()
      .map(|record| RequestRowDto {
        request_id: record.request_id,
        model: record.model,
        elapsed_ms: record.elapsed_ms,
        policy_revision: record.policy_revision,
        project_id: record
          .principal
          .as_ref()
          .map(|principal| principal.project_id.clone()),
        key_id: record
          .principal
          .as_ref()
          .map(|principal| principal.key_id.clone()),
        reserved_tokens: record.reserved_tokens,
        input_tokens: record.usage.map(|usage| usage.input_tokens),
        output_tokens: record.usage.map(|usage| usage.output_tokens),
        cached_input_tokens: record.usage.map(|usage| usage.cached_input_tokens),
        reasoning_tokens: record.usage.map(|usage| usage.reasoning_tokens),
        status: match record.status {
          AuditStatus::Pending => "pending",
          AuditStatus::Succeeded => "succeeded",
          AuditStatus::Failed => "failed",
          AuditStatus::Cancelled => "cancelled",
          AuditStatus::Uncertain => "uncertain",
        }
        .into(),
      })
      .collect(),
  })
}
fn tier(tier: ModelTier) -> &'static str {
  match tier {
    ModelTier::Fast => "fast",
    ModelTier::Balanced => "balanced",
    ModelTier::Advanced => "advanced",
  }
}
