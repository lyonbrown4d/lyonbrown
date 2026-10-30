//! Safe data sent to the browser. Runtime services and provider credentials cannot be represented.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelRowDto {
  pub id: String,
  pub owned_by: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DomainRowDto {
  pub label: String,
  pub tier: String,
  pub terms: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RequestRowDto {
  pub request_id: String,
  pub model: String,
  pub status: String,
  pub elapsed_ms: u64,
  pub policy_revision: String,
  pub project_id: Option<String>,
  pub key_id: Option<String>,
  pub reserved_tokens: u64,
  pub input_tokens: Option<u64>,
  pub output_tokens: Option<u64>,
  pub cached_input_tokens: Option<u64>,
  pub reasoning_tokens: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelsView {
  pub csrf: String,
  pub ready: bool,
  pub models: Vec<ModelRowDto>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PolicyView {
  pub csrf: String,
  pub policy_revision: String,
  pub document: String,
  pub domains: Vec<DomainRowDto>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RequestsView {
  pub csrf: String,
  pub records: Vec<RequestRowDto>,
  pub total: u64,
  pub offset: u64,
  pub limit: u32,
}

/// Metadata only: API key material and token hashes have no representation here.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KeyRowDto {
  pub id: String,
  pub label: String,
  pub project_id: String,
  pub allowed_models: Vec<String>,
  pub max_output_tokens: Option<u64>,
  pub token_budget: Option<u64>,
  pub tokens_charged: u64,
  pub expires_at: u64,
  pub revoked: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KeysView {
  pub csrf: String,
  pub keys: Vec<KeyRowDto>,
}
