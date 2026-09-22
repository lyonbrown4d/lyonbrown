use agent_skill::{Skill, SkillRegistry};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("lyonbrown-skill-test-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn quarkus_skill() -> Skill {
    Skill::from_yaml(
        r#"
name: quarkus-debug
description: Diagnose Quarkus startup failures
triggers: [quarkus, startup failure]
instructions: Inspect the application log before editing configuration.
requires:
  tools: [shell, filesystem]
context:
  files: [pom.xml, application.yaml]
"#,
    )
    .unwrap()
}

#[test]
fn loads_yaml_files_in_stable_filename_order_and_missing_directory_is_empty() {
    let root = TestDir::new();
    let missing = SkillRegistry::load_from_root(root.path().join("missing")).unwrap();
    assert_eq!(missing.iter().count(), 0);

    let skills = root.path().join("skills");
    std::fs::create_dir(&skills).unwrap();
    std::fs::write(skills.join("z.yaml"), "name: zed\ntriggers: [zed]\n").unwrap();
    std::fs::write(skills.join("a.yaml"), "name: alpha\ntriggers: [alpha]\n").unwrap();
    let loaded = SkillRegistry::load_from_root(&skills).unwrap();
    assert_eq!(
        loaded
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "zed"]
    );
}

#[test]
fn filesystem_loader_rejects_duplicate_names_and_oversized_files() {
    let root = TestDir::new();
    std::fs::write(root.path().join("a.yaml"), "name: same\n").unwrap();
    std::fs::write(root.path().join("b.yaml"), "name: same\n").unwrap();
    assert!(
        SkillRegistry::load_from_root(root.path())
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );

    std::fs::remove_file(root.path().join("b.yaml")).unwrap();
    std::fs::write(
        root.path().join("large.yaml"),
        vec![b'x'; agent_skill::MAX_SKILL_FILE_BYTES + 1],
    )
    .unwrap();
    assert!(
        SkillRegistry::load_from_root(root.path())
            .unwrap_err()
            .to_string()
            .contains("exceeds")
    );
}

#[test]
fn filesystem_yaml_errors_include_the_source_filename() {
    let root = TestDir::new();
    std::fs::write(root.path().join("broken.yaml"), "name: [not valid").unwrap();

    let error = SkillRegistry::load_from_root(root.path()).unwrap_err();

    assert!(error.to_string().contains("broken.yaml"));
}

#[test]
fn filesystem_loader_rejects_file_count_and_aggregate_limits() {
    let root = TestDir::new();
    for index in 0..=agent_skill::MAX_SKILL_FILES {
        std::fs::write(
            root.path().join(format!("{index:04}.yaml")),
            format!("name: skill-{index}\n"),
        )
        .unwrap();
    }
    assert!(matches!(
        SkillRegistry::load_from_root(root.path()),
        Err(agent_skill::SkillError::TooManyFiles { .. })
    ));

    let aggregate = TestDir::new();
    let payload = "x".repeat(agent_skill::MAX_AGGREGATE_SKILL_BYTES / 8);
    for index in 0..9 {
        std::fs::write(
            aggregate.path().join(format!("{index}.yaml")),
            format!("name: aggregate-{index}\ndescription: {payload}\n"),
        )
        .unwrap();
    }
    assert!(matches!(
        SkillRegistry::load_from_root(aggregate.path()),
        Err(agent_skill::SkillError::AggregateTooLarge { .. })
    ));
}

#[cfg(unix)]
#[test]
fn filesystem_loader_rejects_symlinked_yaml() {
    use std::os::unix::fs::symlink;
    let root = TestDir::new();
    let outside = root.path().with_extension("outside.yaml");
    std::fs::write(&outside, "name: outside\n").unwrap();
    symlink(&outside, root.path().join("escape.yaml")).unwrap();
    assert!(
        SkillRegistry::load_from_root(root.path())
            .unwrap_err()
            .to_string()
            .contains("link")
    );
    std::fs::remove_file(outside).unwrap();
}

#[cfg(unix)]
#[test]
fn filesystem_loader_rejects_symlinked_ancestor() {
    use std::os::unix::fs::symlink;
    let container = TestDir::new();
    let home = container.path().join("home");
    let outside = container.path().join("outside");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir_all(outside.join("skills")).unwrap();
    symlink(&outside, home.join(".agent")).unwrap();

    assert!(matches!(
        SkillRegistry::load_from_root(home.join(".agent").join("skills")),
        Err(agent_skill::SkillError::Linked(_))
    ));
}

#[cfg(windows)]
#[test]
fn filesystem_loader_rejects_junction_ancestor() {
    use std::process::Command;
    let container = TestDir::new();
    let home = container.path().join("home");
    let outside = container.path().join("outside");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir_all(outside.join("skills")).unwrap();
    let output = Command::new("cmd.exe")
        .args(["/D", "/C", "mklink /J .agent ..\\outside"])
        .current_dir(&home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(matches!(
        SkillRegistry::load_from_root(home.join(".agent").join("skills")),
        Err(agent_skill::SkillError::Linked(_))
    ));
    std::fs::remove_dir(home.join(".agent")).unwrap();
}

#[test]
fn parses_typed_skill_yaml() {
    let skill = quarkus_skill();
    assert_eq!(skill.name, "quarkus-debug");
    assert_eq!(skill.requires.tools, ["shell", "filesystem"]);
    assert_eq!(skill.context.files, ["pom.xml", "application.yaml"]);
}

#[test]
fn matching_is_case_insensitive_and_respects_word_boundaries() {
    let registry = SkillRegistry::new([quarkus_skill()]).unwrap();
    assert_eq!(registry.match_prompt("Debug this QUARKUS service").len(), 1);
    assert!(
        registry
            .match_prompt("Debug the aquarkusify service")
            .is_empty()
    );
    assert!(registry.match_prompt("Unrelated Rust prompt").is_empty());
}

#[test]
fn registry_and_matches_are_deterministic_by_skill_name() {
    let alpha =
        Skill::from_yaml("name: alpha\ntriggers: [service]\ninstructions: alpha\n").unwrap();
    let zeta = Skill::from_yaml("name: zeta\ntriggers: [service]\ninstructions: zeta\n").unwrap();
    let registry = SkillRegistry::new([zeta, alpha]).unwrap();
    let names: Vec<_> = registry
        .match_prompt("service")
        .into_iter()
        .map(|skill| skill.name.as_str())
        .collect();
    assert_eq!(names, ["alpha", "zeta"]);
}

#[test]
fn duplicate_skill_names_are_rejected() {
    let skill = quarkus_skill();
    let error = SkillRegistry::new([skill.clone(), skill]).unwrap_err();
    assert!(error.to_string().contains("duplicate"));
}

#[test]
fn cjk_adjacent_text_is_a_boundary_but_ascii_suffixes_are_not() {
    let registry = SkillRegistry::new([quarkus_skill()]).unwrap();
    assert_eq!(registry.match_prompt("请用quarkus调试").len(), 1);
    assert!(registry.match_prompt("quarkusx").is_empty());
}

#[test]
fn non_ascii_triggers_are_rejected_explicitly() {
    let error = Skill::from_yaml("name: debug\ntriggers: [调试]\n").unwrap_err();
    assert!(error.to_string().contains("ASCII"));
}

#[test]
fn rejects_unknown_top_level_and_nested_skill_fields() {
    let top_level = Skill::from_yaml("name: debug\ntrigers: [debug]\n").unwrap_err();
    assert!(top_level.to_string().contains("trigers"));

    let nested = Skill::from_yaml("name: debug\nrequires:\n  toolz: [shell]\n").unwrap_err();
    assert!(nested.to_string().contains("toolz"));
}
