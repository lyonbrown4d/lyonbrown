use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Result, SkillError};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillManifest {
  pub name: String,
  pub description: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub license: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub compatibility: Option<String>,
  #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
  pub metadata: BTreeMap<String, String>,
  #[serde(
    default,
    rename = "allowed-tools",
    skip_serializing_if = "Option::is_none"
  )]
  pub allowed_tools: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedSkill {
  pub manifest: SkillManifest,
  pub instructions: String,
}

/// Project the external Agent Skills parser into Lyonbrown-owned data types.
/// Declarations remain untrusted data and never grant tool permissions.
pub fn parse_manifest(content: &str) -> Result<ParsedSkill> {
  let content = content.strip_prefix('\u{feff}').unwrap_or(content);
  // Text normalization is performed before the upstream parser. Persisted
  // package bytes and their inventory hashes remain unchanged.
  let normalized = content.replace("\r\n", "\n");
  let skill = agent_skills::Skill::parse(&normalized).map_err(parse_error)?;
  let frontmatter = skill.frontmatter();
  let manifest = SkillManifest {
    name: skill.name().as_str().to_owned(),
    description: skill.description().as_str().to_owned(),
    license: frontmatter.license().map(str::to_owned),
    compatibility: frontmatter
      .compatibility()
      .map(|value| value.as_str().to_owned()),
    metadata: frontmatter
      .metadata()
      .map(|value| {
        value
          .iter()
          .map(|(key, value)| (key.to_owned(), value.to_owned()))
          .collect()
      })
      .unwrap_or_default(),
    allowed_tools: frontmatter.allowed_tools().map(ToString::to_string),
  };
  // Lyonbrown policy adds portable filesystem names to upstream spec validation.
  crate::paths::relative(&manifest.name, 1)?;
  Ok(ParsedSkill {
    manifest,
    instructions: skill.body().to_owned(),
  })
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
  agent_skills::SkillName::new(name)
    .map_err(|_| SkillError::Manifest("invalid Skill name".into()))?;
  // An installed Skill directory must be portable to Windows as well.
  crate::paths::relative(name, 1)?;
  Ok(())
}

fn parse_error(error: agent_skills::ParseError) -> SkillError {
  use agent_skills::ParseError;
  let message = match error {
    ParseError::MissingFrontmatter => "missing YAML frontmatter",
    ParseError::UnterminatedFrontmatter => "unterminated YAML frontmatter",
    ParseError::InvalidYaml { .. } => "invalid YAML fields",
    ParseError::MissingField { .. } => "missing required Skill field",
    ParseError::InvalidName(_) => "invalid Skill name",
    ParseError::InvalidDescription(_) => "invalid Skill description",
    ParseError::InvalidCompatibility(_) => "invalid Skill compatibility",
  };
  SkillError::Manifest(message.into())
}
