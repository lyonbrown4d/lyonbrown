use serde::{Deserialize, Serialize};

pub const MAX_FILES: usize = 32;
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_FILE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PatchInput {
  pub path: String,
  /// Empty means the file must not exist.
  pub expected_revision: String,
  pub patch: String,
  /// Explicit deletion; the resulting patch content must be empty.
  #[serde(default)]
  pub delete: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChangeFile {
  pub path: String,
  pub expected_revision: String,
  pub before: Option<String>,
  pub after: Option<String>,
  pub before_sha256: Option<String>,
  pub after_sha256: Option<String>,
  pub unified_diff: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeState {
  Preview,
  Prepared,
  Applied,
  Uncertain,
}
impl ChangeState {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Preview => "preview",
      Self::Prepared => "prepared",
      Self::Applied => "applied",
      Self::Uncertain => "uncertain",
    }
  }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChangeSet {
  pub id: String,
  pub run_id: String,
  pub owner_epoch: u64,
  pub revision: u64,
  pub state: ChangeState,
  pub preview_sha256: String,
  pub files: Vec<ChangeFile>,
  pub reverses: Option<String>,
  pub action_id: Option<String>,
  /// Files whose replacement returned successfully, not a cross-file transaction.
  pub written_paths: Vec<String>,
  /// Durable host event fence for commands recorded after successful application.
  pub applied_event_cursor: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ValidationRecord {
  pub id: String,
  pub change_id: String,
  pub change_revision: u64,
  pub change_sha256: String,
  /// A host-persisted execute_command operation, never model-supplied status.
  pub operation_id: String,
  /// Command exit evidence associated with a historical change revision only.
  pub passed: bool,
  /// Commands and external editors can mutate files; this never certifies the current tree.
  pub verifies_current_workspace: bool,
  pub exit_code: Option<i64>,
}
