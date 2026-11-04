use agent_changes::{
  MODEL_DIFF_BYTES, MODEL_VALIDATIONS, PatchInput, ValidationRecord, build_preview, model_change,
};
fn preview(after: Vec<(&str, String)>) -> anyhow::Result<agent_changes::ChangeSet> {
  build_preview(
    uuid::Uuid::now_v7().to_string(),
    "run".into(),
    1,
    after
      .into_iter()
      .map(|(path, text)| {
        (
          PatchInput {
            path: path.into(),
            expected_revision: String::new(),
            patch: diffy::create_patch("", &text).to_string(),
            delete: false,
          },
          None,
        )
      })
      .collect(),
  )
}
#[test]
fn model_preview_omits_file_contents_and_bounds_utf8_diffs_without_changing_identity()
-> anyhow::Result<()> {
  let change = preview(vec![
    ("one.txt", "汉".repeat(100_000)),
    ("two.txt", "界".repeat(100_000)),
  ])?;
  let model = model_change(&change, &[], false);
  assert_eq!(model.id, change.id);
  assert_eq!(model.preview_sha256, change.preview_sha256);
  assert!(model.diff_truncated);
  assert!(!model.content_included);
  let included = model
    .files
    .iter()
    .map(|file| file.unified_diff.len())
    .sum::<usize>();
  assert_eq!(included, model.diff_bytes_included);
  assert!(included <= MODEL_DIFF_BYTES);
  assert_eq!(
    model.diff_bytes_total,
    change
      .files
      .iter()
      .map(|file| file.unified_diff.len())
      .sum::<usize>()
  );
  for (file, full) in model.files.iter().zip(&change.files) {
    assert_eq!(file.path, full.path);
    assert_eq!(file.after_sha256, &full.after_sha256);
    assert!(full.unified_diff.starts_with(file.unified_diff));
    assert_eq!(
      file.diff_truncated,
      file.unified_diff.len() < full.unified_diff.len()
    );
  }
  let wire = serde_json::to_value(model)?;
  assert!(
    wire["files"]
      .as_array()
      .expect("files")
      .iter()
      .all(|file| file.get("before").is_none() && file.get("after").is_none())
  );
  assert!(serde_json::to_vec(&wire)?.len() < 256 * 1024);
  assert_eq!(
    change.files[0].after.as_deref(),
    Some("汉".repeat(100_000).as_str())
  );
  Ok(())
}
#[test]
fn small_diffs_and_host_evidence_remain_exact_with_explicit_evidence_truncation()
-> anyhow::Result<()> {
  let change = preview(vec![("small.txt", "content\n".into())])?;
  let evidence = (0..MODEL_VALIDATIONS + 1)
    .map(|index| ValidationRecord {
      id: format!("evidence-{index}"),
      change_id: change.id.clone(),
      change_revision: 3,
      change_sha256: change.preview_sha256.clone(),
      operation_id: format!("operation-{index}"),
      passed: false,
      verifies_current_workspace: false,
      exit_code: Some(1),
    })
    .collect::<Vec<_>>();
  let model = model_change(&change, &evidence, false);
  assert!(!model.diff_truncated);
  assert_eq!(model.files[0].unified_diff, change.files[0].unified_diff);
  assert_eq!(model.host_validations.len(), MODEL_VALIDATIONS);
  assert!(model.host_validations_truncated);
  assert!(!model.model_reported_validation_is_evidence);
  assert!(model.host_validations.iter().all(|record| !record.passed
    && !record.verifies_current_workspace
    && record.exit_code == Some(1)));
  assert!(model_change(&change, &[], true).host_validations_truncated);
  Ok(())
}
