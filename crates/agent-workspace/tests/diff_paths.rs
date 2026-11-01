use agent_workspace::Workspace;

#[test]
fn diff_paths_allow_deleted_files_without_reading_binary_content() {
  let directory = tempfile::tempdir().expect("workspace");
  let workspace = Workspace::open(directory.path()).expect("capability root");
  std::fs::write(directory.path().join("binary"), [0xff, 0x00]).expect("binary fixture");
  for path in [
    "deleted.txt",
    "removed/parent/file",
    "binary",
    "[literal]*.txt",
  ] {
    workspace
      .validate_diff_path(path)
      .expect("relative Git path");
  }
  for path in ["../escape", "/tmp/outside", "src/../../escape", ""] {
    assert!(workspace.validate_diff_path(path).is_err(), "{path}");
  }
}

#[cfg(unix)]
#[test]
fn diff_paths_reject_escaping_symlinks_including_missing_targets() {
  let directory = tempfile::tempdir().expect("workspace");
  let outside = tempfile::tempdir().expect("outside");
  std::os::unix::fs::symlink(outside.path(), directory.path().join("escape")).expect("link");
  let workspace = Workspace::open(directory.path()).expect("capability root");
  assert!(
    workspace
      .validate_diff_path("escape/not-present/file")
      .is_err()
  );
}
