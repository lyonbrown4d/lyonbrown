use std::fs;

use agent_workspace::{ProjectType, WorkspaceProfile, workspace_cache_path};

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
