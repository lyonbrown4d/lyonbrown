use agent_skill::{Skill, SkillRegistry};

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
