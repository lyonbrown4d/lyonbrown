use crate::*;
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::{
  collections::BTreeSet,
  path::{Component, Path},
};

pub fn digest(bytes: &[u8]) -> String {
  hex::encode(Sha256::digest(bytes))
}

/// The host reads each path inside its authorized workspace capability.
pub fn build_preview(
  id: String,
  run_id: String,
  owner_epoch: u64,
  inputs: Vec<(PatchInput, Option<String>)>,
) -> Result<ChangeSet> {
  ensure!(
    !inputs.is_empty() && inputs.len() <= MAX_FILES,
    "change file limit exceeded"
  );
  let mut files = Vec::with_capacity(inputs.len());
  let mut input_bytes = 0usize;
  for (input, before) in inputs {
    input_bytes = input_bytes
      .checked_add(input.patch.len())
      .ok_or_else(|| anyhow::anyhow!("patch byte limit exceeded"))?;
    ensure!(input_bytes <= MAX_BYTES, "patch byte limit exceeded");
    ensure!(
      before.as_deref().unwrap_or("").len() <= MAX_FILE_BYTES,
      "file byte limit exceeded"
    );
    let patch = diffy::Patch::from_str(&input.patch)?;
    let after = diffy::apply(before.as_deref().unwrap_or(""), &patch)?;
    ensure!(
      !input.delete || (before.is_some() && after.is_empty()),
      "deletion requires an existing file and empty after-image"
    );
    files.push(file(
      input.path,
      input.expected_revision,
      before,
      (!input.delete).then_some(after),
    ));
  }
  let mut result = ChangeSet {
    id,
    run_id,
    owner_epoch,
    revision: 1,
    state: ChangeState::Preview,
    preview_sha256: String::new(),
    files,
    reverses: None,
    action_id: None,
    written_paths: Vec::new(),
    applied_event_cursor: None,
  };
  result.preview_sha256 = preview_digest(&result)?;
  validate(&result)?;
  Ok(result)
}

fn file(
  path: String,
  expected_revision: String,
  before: Option<String>,
  after: Option<String>,
) -> ChangeFile {
  ChangeFile {
    path,
    expected_revision,
    before_sha256: before.as_ref().map(|s| digest(s.as_bytes())),
    after_sha256: after.as_ref().map(|s| digest(s.as_bytes())),
    unified_diff: diffy::create_patch(
      before.as_deref().unwrap_or(""),
      after.as_deref().unwrap_or(""),
    )
    .to_string(),
    before,
    after,
  }
}

pub fn reverse_preview(
  id: String,
  run_id: String,
  epoch: u64,
  applied: &ChangeSet,
) -> Result<ChangeSet> {
  ensure!(
    applied.state == ChangeState::Applied,
    "only a completely applied checkpoint can be reversed"
  );
  validate(applied)?;
  let mut result = ChangeSet {
    id,
    run_id,
    owner_epoch: epoch,
    revision: 1,
    state: ChangeState::Preview,
    preview_sha256: String::new(),
    files: applied
      .files
      .iter()
      .map(|f| {
        file(
          f.path.clone(),
          f.after_sha256.clone().unwrap_or_default(),
          f.after.clone(),
          f.before.clone(),
        )
      })
      .collect(),
    reverses: Some(applied.id.clone()),
    action_id: None,
    written_paths: Vec::new(),
    applied_event_cursor: None,
  };
  result.preview_sha256 = preview_digest(&result)?;
  validate(&result)?;
  Ok(result)
}

fn preview_digest(change: &ChangeSet) -> Result<String> {
  Ok(digest(&serde_json::to_vec(&(
    &change.id,
    &change.run_id,
    change.owner_epoch,
    &change.files,
    &change.reverses,
  ))?))
}

pub fn validate(change: &ChangeSet) -> Result<()> {
  uuid::Uuid::parse_str(&change.id)?;
  ensure!(
    !change.run_id.is_empty() && change.revision > 0,
    "invalid change identity"
  );
  ensure!(
    !change.files.is_empty() && change.files.len() <= MAX_FILES,
    "change file limit exceeded"
  );
  let mut paths = BTreeSet::new();
  let mut bytes = 0usize;
  for file in &change.files {
    ensure!(
      !file.path.is_empty()
        && file.path.len() <= 4096
        && !file.path.contains('\\')
        && file
          .path
          .split('/')
          .all(|part| !part.is_empty() && part != "." && part != "..")
        && Path::new(&file.path)
          .components()
          .all(|c| matches!(c, Component::Normal(_))),
      "change paths must be normalized workspace-relative paths"
    );
    ensure!(paths.insert(&file.path), "duplicate change path");
    for content in [&file.before, &file.after].into_iter().flatten() {
      ensure!(content.len() <= MAX_FILE_BYTES, "file byte limit exceeded");
      bytes = bytes
        .checked_add(content.len())
        .ok_or_else(|| anyhow::anyhow!("checkpoint byte limit exceeded"))?;
    }
    bytes = bytes
      .checked_add(file.unified_diff.len())
      .ok_or_else(|| anyhow::anyhow!("checkpoint byte limit exceeded"))?;
    ensure!(bytes <= MAX_BYTES, "checkpoint byte limit exceeded");
    ensure!(
      file.before.is_some() || file.after.is_some(),
      "empty change"
    );
    ensure!(
      file.before.as_ref().map(|s| digest(s.as_bytes())) == file.before_sha256
        && file.after.as_ref().map(|s| digest(s.as_bytes())) == file.after_sha256,
      "checkpoint hash mismatch"
    );
    ensure!(
      file.expected_revision == file.before_sha256.clone().unwrap_or_default(),
      "expected revision mismatch"
    );
    ensure!(
      file.unified_diff
        == diffy::create_patch(
          file.before.as_deref().unwrap_or(""),
          file.after.as_deref().unwrap_or("")
        )
        .to_string(),
      "checkpoint diff mismatch"
    );
  }
  ensure!(
    change.preview_sha256 == preview_digest(change)?,
    "preview hash mismatch"
  );
  Ok(())
}
