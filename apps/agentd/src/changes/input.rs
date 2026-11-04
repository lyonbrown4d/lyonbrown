use agent_changes::PatchInput;
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Preview {
  pub request_id: String,
  pub files: Vec<PatchInput>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Read {
  pub change_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Undo {
  pub request_id: String,
  pub change_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Apply {
  pub request_id: String,
  pub change_id: String,
  pub expected_revision: u64,
  pub preview_sha256: String,
  pub action_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Validation {
  pub request_id: String,
  pub change_id: String,
  pub expected_revision: u64,
  pub operation_id: String,
}
