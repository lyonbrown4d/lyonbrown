use std::fs;

use agent_workspace::{
  MAX_AGGREGATE_INSTRUCTION_BYTES, MAX_INSTRUCTION_FILE_BYTES, ProjectType, WorkspaceProfile,
  load_repository_instructions, workspace_cache_path, workspace_identity,
};

fn fixture_root(name: &str) -> std::path::PathBuf {
  let root = std::env::temp_dir().join(format!("agent-workspace-{name}-{}", std::process::id()));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).unwrap();
  root
}

#[test]
fn detects_a_fresh_cargo_repository_without_configuration() {
  let root = fixture_root("cargo");
  fs::write(
    root.join("Cargo.toml"),
    "[package]\nname = \"fresh\"\nversion = \"0.1.0\"\n",
  )
  .unwrap();

  let profile = WorkspaceProfile::detect(&root).unwrap();
  assert_eq!(profile.project_type, ProjectType::Rust);
  assert_eq!(profile.languages, ["Rust"]);
  assert_eq!(profile.build_commands, ["cargo build"]);
  assert_eq!(profile.test_commands, ["cargo test"]);
  fs::remove_dir_all(root).unwrap();
}

#[test]
fn loads_repository_instructions_in_canonical_order() {
  let root = fixture_root("instructions");
  for (name, body) in [
    ("CONTRIBUTING.md", "contributing"),
    ("README.md", "readme"),
    ("CLAUDE.md", "claude"),
    ("AGENTS.md", "agents"),
  ] {
    fs::write(root.join(name), body).unwrap();
  }

  let profile = WorkspaceProfile::detect(&root).unwrap();
  let names: Vec<_> = profile
    .repository_instructions
    .iter()
    .map(|instruction| instruction.path.as_str())
    .collect();
  assert_eq!(
    names,
    ["AGENTS.md", "CLAUDE.md", "README.md", "CONTRIBUTING.md"]
  );
  assert_eq!(profile.repository_instructions[0].content, "agents");
  fs::remove_dir_all(root).unwrap();
}

#[test]
fn cache_path_is_stable_and_outside_the_repository() {
  let root = fixture_root("cache-repo");
  let home = fixture_root("cache-home");
  let first = workspace_cache_path(&home, &root).unwrap();
  let second = workspace_cache_path(&home, &root).unwrap();
  assert_eq!(first, second);
  assert!(first.starts_with(home.join(".agent/workspaces")));
  assert!(!first.starts_with(&root));
  assert!(!first.exists(), "computing a cache path must not write it");
  fs::remove_dir_all(root).unwrap();
  fs::remove_dir_all(home).unwrap();
}

#[cfg(windows)]
#[test]
fn workspace_identity_and_cache_ignore_windows_path_casing() {
  let root = fixture_root("windows-identity");
  let home = fixture_root("windows-identity-home");
  let alternate_case = std::path::PathBuf::from(root.to_string_lossy().to_ascii_uppercase());

  assert_eq!(
    workspace_identity(&root).unwrap(),
    workspace_identity(&alternate_case).unwrap()
  );
  assert_eq!(
    workspace_cache_path(&home, &root).unwrap(),
    workspace_cache_path(&home, &alternate_case).unwrap()
  );

  fs::remove_dir_all(root).unwrap();
  fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[test]
fn workspace_identity_and_cache_follow_directory_aliases() {
  let root = fixture_root("unix-identity");
  let home = fixture_root("unix-identity-home");
  let alias = root.with_file_name(format!(
    "agent-workspace-unix-identity-alias-{}",
    std::process::id()
  ));
  let _ = fs::remove_file(&alias);
  std::os::unix::fs::symlink(&root, &alias).unwrap();

  assert_eq!(
    workspace_identity(&root).unwrap(),
    workspace_identity(&alias).unwrap()
  );
  assert_eq!(
    workspace_cache_path(&home, &root).unwrap(),
    workspace_cache_path(&home, &alias).unwrap()
  );

  fs::remove_file(alias).unwrap();
  fs::remove_dir_all(root).unwrap();
  fs::remove_dir_all(home).unwrap();
}

#[test]
fn rejects_an_oversized_instruction_file_without_loading_it() {
  let root = fixture_root("oversized");
  fs::write(
    root.join("AGENTS.md"),
    vec![b'x'; MAX_INSTRUCTION_FILE_BYTES + 1],
  )
  .unwrap();

  let error = load_repository_instructions(&root).unwrap_err();
  assert!(error.to_string().contains("exceeds"));
  fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_instruction_files_over_the_aggregate_limit() {
  let root = fixture_root("aggregate");
  let each = MAX_AGGREGATE_INSTRUCTION_BYTES / 3;
  for name in ["AGENTS.md", "CLAUDE.md", "README.md", "CONTRIBUTING.md"] {
    fs::write(root.join(name), vec![b'x'; each]).unwrap();
  }

  let error = load_repository_instructions(&root).unwrap_err();
  assert!(error.to_string().contains("aggregate"));
  fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn rejects_repository_instruction_symlink_escape() {
  let root = fixture_root("link-root");
  let outside = fixture_root("link-outside");
  fs::write(outside.join("secret.md"), "outside secret").unwrap();
  std::os::unix::fs::symlink(outside.join("secret.md"), root.join("AGENTS.md")).unwrap();

  assert!(load_repository_instructions(&root).is_err());
  fs::remove_dir_all(root).unwrap();
  fs::remove_dir_all(outside).unwrap();
}

#[cfg(windows)]
#[test]
fn rejects_repository_instruction_junction_escape() {
  use std::process::Command;

  let container = fixture_root("junction-container");
  let outside = container.join("outside");
  let workspace = container.join("workspace");
  fs::create_dir(&outside).unwrap();
  fs::write(outside.join("AGENTS.md"), "outside secret").unwrap();
  let output = Command::new("cmd.exe")
    .args(["/D", "/C", "mklink /J workspace outside"])
    .current_dir(&container)
    .output()
    .unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );

  assert!(load_repository_instructions(&workspace).is_err());
  fs::remove_dir(&workspace).unwrap();
  fs::remove_dir_all(container).unwrap();
}
