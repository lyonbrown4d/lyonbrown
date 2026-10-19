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
