use super::{
  auth,
  errors::{WebError, redirect},
};
use crate::{
  dto::{KeyRowDto, KeysView},
  key_api::OneTimeToken,
  key_form::{KeyDraft, limit},
};
use axum::http::StatusCode;
use garde::Validate;
use gateway_storage::StorageError;
use leptos::prelude::*;

pub(crate) async fn list() -> Result<KeysView, ServerFnError> {
  let (state, headers) = auth::context().map_err(WebError::server_fn)?;
  let (_, csrf) = auth::authenticated(&state, &headers)
    .await
    .map_err(WebError::server_fn)?;
  let keys = state
    .store
    .list_access_tokens()
    .await
    .map_err(|_| WebError::unavailable().server_fn())?;
  Ok(KeysView {
    csrf,
    keys: keys
      .into_iter()
      .map(|key| KeyRowDto {
        id: key.id,
        label: key.label,
        project_id: key.project_id,
        allowed_models: key.allowed_models.into_iter().collect(),
        max_output_tokens: key.max_output_tokens,
        token_budget: key.token_budget,
        tokens_charged: key.tokens_charged,
        expires_at: key.expires_at,
        revoked: key.revoked,
      })
      .collect(),
  })
}
pub(crate) async fn create(
  csrf: String,
  draft: KeyDraft<'_>,
) -> Result<OneTimeToken, ServerFnError> {
  let (state, headers) = auth::context().map_err(WebError::server_fn)?;
  auth::verify_post(&state, &headers, &csrf)
    .await
    .map_err(WebError::server_fn)?;
  let invalid = || {
    WebError {
      status: StatusCode::BAD_REQUEST,
      message: "API Key 参数无效，请检查范围和有效期。",
    }
    .server_fn()
  };
  draft.validate().map_err(|_| invalid())?;
  let output = limit(draft.max_output_tokens).map_err(|_| invalid())?;
  let budget = limit(draft.token_budget).map_err(|_| invalid())?;
  let key = state
    .store
    .create_scoped_access_token(
      draft.label,
      auth::now().map_err(WebError::server_fn)?,
      draft.ttl_seconds,
      draft.project_id,
      draft.models(),
      output,
      budget,
    )
    .await
    .map_err(|error| match error {
      StorageError::InvalidInput(_) => invalid(),
      _ => WebError::unavailable().server_fn(),
    })?;
  Ok(OneTimeToken::new(key.token().to_owned()))
}
pub(crate) async fn revoke(csrf: &str, id: &str) -> Result<(), ServerFnError> {
  let (state, headers) = auth::context().map_err(WebError::server_fn)?;
  auth::verify_post(&state, &headers, csrf)
    .await
    .map_err(WebError::server_fn)?;
  state
    .store
    .revoke_access_token(id)
    .await
    .map_err(|error| match error {
      StorageError::InvalidInput(_) => WebError {
        status: StatusCode::BAD_REQUEST,
        message: "API Key 标识无效。",
      }
      .server_fn(),
      _ => WebError::unavailable().server_fn(),
    })?;
  redirect("/ui/keys?revoked=true", &headers);
  Ok(())
}
