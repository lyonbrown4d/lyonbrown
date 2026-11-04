use agent_changes::{
  ChangeState, MAX_FILES, PatchInput, build_preview, digest, reverse_preview, validate,
};
fn patch(
  path: &str,
  before: Option<&str>,
  after: &str,
  delete: bool,
) -> (PatchInput, Option<String>) {
  (
    PatchInput {
      path: path.into(),
      expected_revision: before.map(|s| digest(s.as_bytes())).unwrap_or_default(),
      patch: diffy::create_patch(before.unwrap_or(""), after).to_string(),
      delete,
    },
    before.map(String::from),
  )
}
fn preview(files: Vec<(PatchInput, Option<String>)>) -> anyhow::Result<agent_changes::ChangeSet> {
  build_preview(uuid::Uuid::now_v7().to_string(), "run".into(), 1, files)
}
#[test]
fn mixed_changes_have_reviewable_images_and_exact_reversal() -> anyhow::Result<()> {
  let mut change = preview(vec![
    patch("edit", Some("old\n"), "new\n", false),
    patch("new", None, "created", false),
    patch("removed", Some("deleted"), "", true),
  ])?;
  assert_eq!(change.files[0].before_sha256, Some(digest(b"old\n")));
  assert_eq!(change.files[0].after_sha256, Some(digest(b"new\n")));
  assert!(change.files[1].before.is_none());
  assert!(change.files[2].after.is_none());
  assert!(reverse_preview(uuid::Uuid::now_v7().to_string(), "run".into(), 1, &change).is_err());
  change.state = ChangeState::Applied;
  let undo = reverse_preview(
    uuid::Uuid::now_v7().to_string(),
    "next-run".into(),
    1,
    &change,
  )?;
  for (original, reversed) in change.files.iter().zip(&undo.files) {
    assert_eq!(original.after, reversed.before);
    assert_eq!(original.before, reversed.after);
    assert_eq!(
      reversed.expected_revision,
      original.after_sha256.clone().unwrap_or_default()
    );
  }
  assert_eq!(undo.reverses, Some(change.id));
  Ok(())
}
#[test]
fn stale_revisions_duplicate_paths_escape_and_limits_are_rejected() -> anyhow::Result<()> {
  let mut stale = patch("file", Some("original"), "replacement", false);
  stale.0.expected_revision = digest(b"stale");
  assert!(preview(vec![stale]).is_err());
  assert!(
    preview(vec![
      patch("same", None, "x", false),
      patch("same", None, "y", false)
    ])
    .is_err()
  );
  for path in ["../outside", "/absolute", "./alias", ""] {
    assert!(preview(vec![patch(path, None, "x", false)]).is_err());
  }
  assert!(
    preview(
      (0..=MAX_FILES)
        .map(|n| patch(&format!("file{n}"), None, "x", false))
        .collect()
    )
    .is_err()
  );
  let mut changed = preview(vec![patch("file", None, "x", false)])?;
  changed.files[0].after = Some("tampered".into());
  assert!(validate(&changed).is_err());
  Ok(())
}
