use super::{
  WebState, auth,
  errors::{WebError, redirect},
};
use axum::http::StatusCode;
use decision_core::{Policy, PolicySnapshot};
use gateway_storage::StorageError;
use gateway_validation::PolicyDraft;
use leptos::prelude::*;
use std::sync::Arc;

pub(crate) async fn save(
  csrf: String,
  expected_revision: String,
  revision: String,
  document: String,
) -> Result<(), ServerFnError> {
  let (state, headers) = auth::context().map_err(WebError::server_fn)?;
  auth::verify_post(&state, &headers, &csrf)
    .await
    .map_err(WebError::server_fn)?;
  let invalid = || {
    WebError {
      status: StatusCode::UNPROCESSABLE_ENTITY,
      message: "策略校验失败。请检查 JSON 字段范围，并使用新的版本名称。",
    }
    .server_fn()
  };
  if (PolicyDraft {
    expected_revision: &expected_revision,
    revision: &revision,
    document: &document,
  })
  .validate()
  .is_err()
  {
    return Err(invalid());
  }
  let mut candidate: Policy = serde_json::from_str(&document).map_err(|_| invalid())?;
  candidate.revision = revision;
  if PolicySnapshot::new(candidate.clone()).is_err() {
    return Err(invalid());
  }
  // Committing the DB and publishing the snapshot must finish if the HTTP peer disconnects.
  let outcome =
    tokio::spawn(async move { apply_policy(Arc::new(state), candidate, expected_revision).await })
      .await;
  match outcome {
    Ok(Ok(())) => {
      redirect("/ui/policy?saved=true", &headers);
      Ok(())
    }
    Ok(Err(StorageError::Conflict)) => Err(
      WebError {
        status: StatusCode::CONFLICT,
        message: "策略已发生变化。请刷新页面后重新编辑。",
      }
      .server_fn(),
    ),
    _ => Err(WebError::unavailable().server_fn()),
  }
}
async fn apply_policy(
  state: Arc<WebState>,
  candidate: Policy,
  expected: String,
) -> Result<(), StorageError> {
  let _writer = state.policy_write.lock().await;
  let previous = state.engine.snapshot().policy().clone();
  if previous.revision != expected {
    return Err(StorageError::Conflict);
  }
  state
    .store
    .save_policy(candidate.clone(), Some(expected))
    .await?;
  if state.engine.replace_policy(candidate.clone()).is_err() {
    if state
      .store
      .save_policy(previous, Some(candidate.revision))
      .await
      .is_err()
    {
      state.gateway.set_ready(false);
    }
    return Err(StorageError::Worker);
  }
  Ok(())
}
