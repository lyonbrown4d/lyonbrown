use crate::dto::{ModelsView, PolicyView, RequestsView};
use leptos::{prelude::*, server_fn::codec::GetUrl};
use serde::{Deserialize, Serialize};

/// Transparent form input whose generated server-function Debug never reveals the password.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct PasswordInput(String);
impl std::fmt::Debug for PasswordInput {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("[REDACTED]")
  }
}
impl From<String> for PasswordInput {
  fn from(value: String) -> Self {
    Self(value)
  }
}

#[server(Login, prefix = "/ui", endpoint = "login")]
pub async fn login(
  #[server(default)] username: String,
  #[server(default)] password: PasswordInput,
) -> Result<(), ServerFnError> {
  crate::server::actions::login(&username, &password.0).await
}
#[server(Logout, prefix = "/ui", endpoint = "logout")]
pub async fn logout(csrf: String) -> Result<(), ServerFnError> {
  crate::server::actions::logout(&csrf).await
}
#[server(SavePolicy, prefix = "/ui", endpoint = "policy")]
pub async fn save_policy(
  csrf: String,
  expected_revision: String,
  revision: String,
  document: String,
) -> Result<(), ServerFnError> {
  crate::server::policy::save(csrf, expected_revision, revision, document).await
}
#[server(GetModels, prefix = "/ui/api", endpoint = "models", input = GetUrl)]
pub async fn get_models() -> Result<ModelsView, ServerFnError> {
  crate::server::data::models().await
}
#[server(GetPolicy, prefix = "/ui/api", endpoint = "policy", input = GetUrl)]
pub async fn get_policy() -> Result<PolicyView, ServerFnError> {
  crate::server::data::policy().await
}
#[server(GetRequests, prefix = "/ui/api", endpoint = "requests", input = GetUrl)]
pub async fn get_requests(offset: u64, limit: u32) -> Result<RequestsView, ServerFnError> {
  crate::server::data::requests(offset, limit).await
}
