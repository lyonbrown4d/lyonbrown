use agent_workspace::Workspace;
#[test]
fn removing_a_created_file_requires_its_current_after_image() -> anyhow::Result<()> {
  let root = tempfile::tempdir()?;
  let workspace = Workspace::open(root.path())?;
  let applied = workspace.write_revision("new.txt", "applied", "")?;
  workspace.write_revision("new.txt", "user edit", &applied)?;
  assert!(workspace.remove_revision("new.txt", &applied).is_err());
  assert_eq!(workspace.read("new.txt")?, "user edit");
  let (_, current) = workspace.read_revision("new.txt")?;
  workspace.remove_revision("new.txt", &current)?;
  assert!(!root.path().join("new.txt").exists());
  assert!(workspace.remove_revision("missing", "").is_err());
  assert!(workspace.remove_revision("../outside", &current).is_err());
  Ok(())
}
#[cfg(unix)]
#[test]
fn patch_paths_reject_symlink_aliases_without_deleting_the_target() -> anyhow::Result<()> {
  let root = tempfile::tempdir()?;
  let workspace = Workspace::open(root.path())?;
  let revision = workspace.write_revision("real/file", "original", "")?;
  std::os::unix::fs::symlink("real", root.path().join("alias"))?;
  assert!(workspace.validate_change_path("alias/file").is_err());
  assert!(workspace.remove_revision("alias/file", &revision).is_err());
  assert_eq!(workspace.read("real/file")?, "original");
  Ok(())
}
