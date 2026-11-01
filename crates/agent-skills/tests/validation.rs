use lyonbrown_skills::{SkillSource, parse_manifest};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn upstream_validation_projects_normalized_instructions_without_granting_tools() -> TestResult {
  let parsed = parse_manifest(
    "---\r\nname: review\r\ndescription: Review changes\r\nallowed-tools: '*'\r\n---\r\nInstructions\r\n",
  )?;
  assert_eq!(parsed.instructions, "Instructions\n");
  assert_eq!(parsed.manifest.allowed_tools.as_deref(), Some("*"));
  for content in [
    "name: review\ndescription: no frontmatter",
    "---\nname: Review\ndescription: invalid name\n---\n",
    "---\nname: review\ndescription: ''\n---\n",
    "---\nname: review\ndescription: test\ncompatibility: ''\n---\n",
    "---\nname: review\nname: other\ndescription: duplicate\n---\n",
    "---\nname: review\ndescription: test\nmetadata:\n  version: [1,2]\n---\n",
  ] {
    assert!(
      parse_manifest(content).is_err(),
      "invalid manifest was accepted"
    );
  }
  assert!(parse_manifest("---\nname: révision\ndescription: Review changes\n---\n").is_err());
  let projected = parse_manifest(
    "\u{feff}---\r\nname: review\r\ndescription: 检查变更\r\nlicense: MIT\r\ncompatibility: Requires cargo\r\nmetadata:\r\n  author: example\r\n  version: '1.0'\r\nallowed-tools: 'Read   Bash(*)'\r\n---\r\n检查代码。\r\n",
  )?;
  assert_eq!(projected.instructions, "检查代码。\n");
  assert_eq!(projected.manifest.description, "检查变更");
  assert_eq!(projected.manifest.license.as_deref(), Some("MIT"));
  assert_eq!(
    projected.manifest.compatibility.as_deref(),
    Some("Requires cargo")
  );
  assert_eq!(
    projected
      .manifest
      .metadata
      .get("author")
      .map(String::as_str),
    Some("example")
  );
  assert_eq!(
    projected
      .manifest
      .metadata
      .get("version")
      .map(String::as_str),
    Some("1.0")
  );
  assert_eq!(
    projected.manifest.allowed_tools.as_deref(),
    Some("Read Bash(*)")
  );
  Ok(())
}

#[test]
fn source_resolution_is_inert_and_explicit() -> TestResult {
  assert!(matches!(
    SkillSource::parse("example/skills")?,
    SkillSource::GitHub { .. }
  ));
  assert!(matches!(
    SkillSource::parse("https://github.com/example/skills/tree/abc123/rust-review")?,
    SkillSource::GitHub {
      revision: Some(_),
      directory: Some(_),
      ..
    }
  ));
  assert!(matches!(
    SkillSource::parse("./local")?,
    SkillSource::Local { .. }
  ));
  assert!(matches!(
    SkillSource::parse("https://example.com/skills.zip")?,
    SkillSource::Https { .. }
  ));
  for source in [
    "http://example.com/a",
    "file:///etc/passwd",
    "https://user:secret@example.com/a",
    "https://example.com/a?token=secret",
    "../repo\n",
  ] {
    assert!(SkillSource::parse(source).is_err());
  }
  Ok(())
}
