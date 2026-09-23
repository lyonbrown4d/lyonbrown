use std::fs;

use agent_profile::{AgentProfile, ProfileLoader};

fn fixture_root(name: &str) -> std::path::PathBuf {
  let root = std::env::temp_dir().join(format!("agent-profile-{name}-{}", std::process::id()));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(root.join(".agent/profiles")).unwrap();
  root
}

#[test]
fn loads_and_validates_typed_yaml() {
  let profile = AgentProfile::from_yaml(
    r#"
id: careful-dev
persona: Careful software engineer
working_style:
  inspect_before_editing: true
  run_tests_after_changes: true
model:
  prefer_local: true
coding:
  prefer_small_reviewable_changes: true
default_skills: [rust]
"#,
  )
  .unwrap();

  assert_eq!(profile.id, "careful-dev");
  assert!(profile.working_style.inspect_before_editing);
  assert!(profile.coding.prefer_small_reviewable_changes);
  assert_eq!(profile.default_skills, ["rust"]);
}

#[test]
fn rejects_profiles_without_an_id() {
  let error = AgentProfile::from_yaml("persona: nobody\n").unwrap_err();
  assert!(error.to_string().contains("id"));
}

#[test]
fn has_a_valid_builtin_default() {
  let profile = AgentProfile::builtin_default();
  profile.validate().unwrap();
  assert_eq!(profile.id, "default-dev");
  assert!(profile.working_style.inspect_before_editing);
}

#[test]
fn loads_named_profile_from_an_explicit_home_root() {
  let root = fixture_root("explicit-root");
  fs::write(
    root.join(".agent/profiles/team.yaml"),
    "id: team\npersona: Team developer\n",
  )
  .unwrap();

  let loaded = ProfileLoader::new(&root).load("team").unwrap();
  assert_eq!(loaded.id, "team");
  assert_eq!(loaded.persona.as_deref(), Some("Team developer"));
  fs::remove_dir_all(root).unwrap();
}

#[test]
fn profile_ids_cannot_escape_the_profiles_directory() {
  let root = fixture_root("escape");
  let error = ProfileLoader::new(&root).load("../secret").unwrap_err();
  assert!(error.to_string().contains("profile id"));
  fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_unknown_top_level_and_nested_profile_fields() {
  let top_level = AgentProfile::from_yaml("id: dev\npersonna: typo\n").unwrap_err();
  assert!(top_level.to_string().contains("personna"));

  let nested =
    AgentProfile::from_yaml("id: dev\nworking_style:\n  inspect_before_editting: true\n")
      .unwrap_err();
  assert!(nested.to_string().contains("inspect_before_editting"));
}
