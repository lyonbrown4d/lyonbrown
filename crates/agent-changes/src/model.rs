//! Bounded model-facing view. Full checkpoint images remain private host state.
use crate::{ChangeSet, ChangeState, ValidationRecord};
use serde::Serialize;
pub const MODEL_DIFF_BYTES: usize = 128 * 1024;
pub const MODEL_VALIDATIONS: usize = 64;
#[derive(Debug, Serialize)]
pub struct ModelChange<'a> {
  pub id: &'a str,
  pub run_id: &'a str,
  pub owner_epoch: u64,
  pub revision: u64,
  pub state: ChangeState,
  pub preview_sha256: &'a str,
  pub reverses: &'a Option<String>,
  pub action_id: &'a Option<String>,
  pub written_paths: &'a [String],
  pub files: Vec<ModelChangeFile<'a>>,
  pub content_included: bool,
  pub diff_bytes_total: usize,
  pub diff_bytes_included: usize,
  pub diff_truncated: bool,
  pub full_diff_available_via: &'static str,
  pub host_validations: &'a [ValidationRecord],
  pub host_validations_truncated: bool,
  pub model_reported_validation_is_evidence: bool,
}
#[derive(Debug, Serialize)]
pub struct ModelChangeFile<'a> {
  pub path: &'a str,
  pub expected_revision: &'a str,
  pub before_sha256: &'a Option<String>,
  pub after_sha256: &'a Option<String>,
  pub unified_diff: &'a str,
  pub diff_bytes_total: usize,
  pub diff_truncated: bool,
}
/// Diff slices are UTF-8 prefixes, never partial digests or fabricated complete
/// files. A missing diff tail is stated even when no budget remains for a file.
pub fn model_change<'a>(
  change: &'a ChangeSet,
  validations: &'a [ValidationRecord],
  more_validations: bool,
) -> ModelChange<'a> {
  let mut remaining = MODEL_DIFF_BYTES;
  let mut diff_bytes_total = 0usize;
  let mut diff_bytes_included = 0usize;
  let mut diff_truncated = false;
  let files = change
    .files
    .iter()
    .map(|file| {
      let mut end = file.unified_diff.len().min(remaining);
      while !file.unified_diff.is_char_boundary(end) {
        end -= 1;
      }
      remaining -= end;
      diff_bytes_total = diff_bytes_total.saturating_add(file.unified_diff.len());
      diff_bytes_included += end;
      let truncated = end < file.unified_diff.len();
      diff_truncated |= truncated;
      ModelChangeFile {
        path: &file.path,
        expected_revision: &file.expected_revision,
        before_sha256: &file.before_sha256,
        after_sha256: &file.after_sha256,
        unified_diff: &file.unified_diff[..end],
        diff_bytes_total: file.unified_diff.len(),
        diff_truncated: truncated,
      }
    })
    .collect();
  ModelChange {
    id: &change.id,
    run_id: &change.run_id,
    owner_epoch: change.owner_epoch,
    revision: change.revision,
    state: change.state,
    preview_sha256: &change.preview_sha256,
    reverses: &change.reverses,
    action_id: &change.action_id,
    written_paths: &change.written_paths,
    files,
    content_included: false,
    diff_bytes_total,
    diff_bytes_included,
    diff_truncated,
    full_diff_available_via: "workflow.get_change",
    host_validations: &validations[..validations.len().min(MODEL_VALIDATIONS)],
    host_validations_truncated: more_validations || validations.len() > MODEL_VALIDATIONS,
    model_reported_validation_is_evidence: false,
  }
}
