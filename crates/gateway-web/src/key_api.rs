//! Authenticated key issuance is the only response allowed to contain an API key.
use crate::dto::KeysView;
use leptos::{prelude::*, server_fn::codec::GetUrl};
use serde::{Deserialize, Serialize};

/// A one-time issuance response. Never use this type for lists, URLs, or persistent state.
#[derive(Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct OneTimeToken(String);
impl std::fmt::Debug for OneTimeToken {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("[REDACTED]")
  }
}
impl OneTimeToken {
  #[cfg(feature = "ssr")]
  pub(crate) fn new(value: String) -> Self {
    Self(value)
  }
  pub(crate) fn display_once(&self) -> &str {
    &self.0
  }
}

#[server(GetKeys, prefix = "/ui/api", endpoint = "keys", input = GetUrl)]
pub async fn get_keys() -> Result<KeysView, ServerFnError> {
  crate::server::keys::list().await
}
#[allow(clippy::too_many_arguments)]
#[server(CreateKey, prefix = "/ui", endpoint = "keys/create")]
pub async fn create_key(
  csrf: String,
  label: String,
  project_id: String,
  allowed_models: String,
  max_output_tokens: String,
  token_budget: String,
  ttl_seconds: u64,
) -> Result<OneTimeToken, ServerFnError> {
  crate::server::keys::create(
    csrf,
    crate::key_form::KeyDraft {
      label: &label,
      project_id: &project_id,
      allowed_models: &allowed_models,
      max_output_tokens: &max_output_tokens,
      token_budget: &token_budget,
      ttl_seconds,
    },
  )
  .await
}
#[server(RevokeKey, prefix = "/ui", endpoint = "keys/revoke")]
pub async fn revoke_key(csrf: String, id: String) -> Result<(), ServerFnError> {
  crate::server::keys::revoke(&csrf, &id).await
}
