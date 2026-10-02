mod support;

use agent_launch::{LaunchError, LaunchRequest};
use std::path::PathBuf;
use std::time::Duration;
use support::workspace;

#[test]
fn rejects_non_canonical_relative_workspace() {
  let error = LaunchRequest::new(
    PathBuf::from("relative/workspace"),
    "runtime-config-v1",
    true,
    Duration::from_secs(1),
  )
  .unwrap_err();

  assert!(matches!(error, LaunchError::InvalidRequest(_)));
}

#[test]
fn rejects_empty_runtime_fingerprint() {
  let error = LaunchRequest::new(workspace(), "", true, Duration::from_secs(1)).unwrap_err();

  assert!(matches!(error, LaunchError::InvalidRequest(_)));
}

#[test]
fn same_workspace_and_runtime_fingerprint_produce_same_scope() {
  let first = LaunchRequest::new(
    workspace(),
    "runtime-config-v1",
    true,
    Duration::from_secs(1),
  )
  .unwrap();
  let second = LaunchRequest::new(
    workspace(),
    "runtime-config-v1",
    false,
    Duration::from_secs(2),
  )
  .unwrap();

  assert_eq!(first.scope_fingerprint(), second.scope_fingerprint());
}

#[cfg(windows)]
#[test]
fn equivalent_windows_path_casing_produces_the_same_scope() {
  let directory = tempfile::tempdir().unwrap();
  let workspace = directory.path().canonicalize().unwrap();
  let alternate_case = PathBuf::from(workspace.to_string_lossy().to_ascii_uppercase());
  let first =
    LaunchRequest::new(workspace, "runtime-config-v1", true, Duration::from_secs(1)).unwrap();
  let second = LaunchRequest::new(
    alternate_case,
    "runtime-config-v1",
    true,
    Duration::from_secs(1),
  )
  .unwrap();

  assert_eq!(first.scope_fingerprint(), second.scope_fingerprint());
}

#[cfg(unix)]
#[test]
fn equivalent_unix_directory_aliases_produce_the_same_scope() {
  let container = tempfile::tempdir().unwrap();
  let workspace = container.path().join("workspace");
  let alias = container.path().join("alias");
  std::fs::create_dir(&workspace).unwrap();
  std::os::unix::fs::symlink(&workspace, &alias).unwrap();
  let first =
    LaunchRequest::new(workspace, "runtime-config-v1", true, Duration::from_secs(1)).unwrap();
  let second =
    LaunchRequest::new(alias, "runtime-config-v1", true, Duration::from_secs(1)).unwrap();

  assert_eq!(first.scope_fingerprint(), second.scope_fingerprint());
}
