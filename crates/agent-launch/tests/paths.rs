use std::path::Path;

use agent_launch::{canonicalize_optional_path, scoped_run_directory};
use agent_local::ScopeFingerprint;

#[test]
fn explicit_paths_are_canonicalized_before_process_handoff() {
  let temporary = tempfile::tempdir().expect("temporary root");
  let config_directory = temporary.path().join("config");
  std::fs::create_dir_all(&config_directory).expect("config directory");
  let config = config_directory.join("agent.toml");
  std::fs::write(&config, "").expect("config file");
  let unresolved = config_directory
    .join("..")
    .join("config")
    .join("agent.toml");

  let resolved = canonicalize_optional_path(Some(&unresolved))
    .expect("canonical path")
    .expect("present path");

  assert!(resolved.is_absolute());
  assert_eq!(resolved, config.canonicalize().expect("expected path"));
}

#[test]
fn run_directories_are_isolated_by_runtime_scope() {
  let temporary = tempfile::tempdir().expect("temporary root");
  let home = temporary.path().join("home");
  let workspace = temporary.path().join("workspace");
  std::fs::create_dir_all(&home).expect("home directory");
  std::fs::create_dir_all(&workspace).expect("workspace directory");
  let first = ScopeFingerprint::parse(&"11".repeat(32)).expect("first scope");
  let second = ScopeFingerprint::parse(&"22".repeat(32)).expect("second scope");

  let first_path = scoped_run_directory(&home, &workspace, &first).expect("first path");
  let second_path = scoped_run_directory(&home, &workspace, &second).expect("second path");

  assert_ne!(first_path, second_path);
  assert_eq!(
    first_path.file_name(),
    Some(Path::new(first.as_str()).as_os_str())
  );
  assert_eq!(
    first_path.parent().and_then(Path::file_name),
    Some(Path::new("run").as_os_str())
  );
}
