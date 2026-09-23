//! Typed skills, deterministic registration, and trigger matching.

use std::{
  collections::BTreeMap,
  fs,
  io::Read,
  path::{Path, PathBuf},
};

use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
  pub name: String,
  #[serde(default)]
  pub description: String,
  #[serde(default)]
  pub triggers: Vec<String>,
  #[serde(default)]
  pub instructions: String,
  #[serde(default)]
  pub requires: SkillRequirements,
  #[serde(default)]
  pub context: SkillContext,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillRequirements {
  #[serde(default)]
  pub tools: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillContext {
  #[serde(default)]
  pub files: Vec<String>,
}

/// Maximum accepted size of one skill file (256 KiB).
pub const MAX_SKILL_FILE_BYTES: usize = 256 * 1024;
/// Maximum number of skill files loaded from one directory.
pub const MAX_SKILL_FILES: usize = 128;
/// Maximum accepted total size of all skill files (1 MiB).
pub const MAX_AGGREGATE_SKILL_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum SkillError {
  #[error("invalid skill YAML: {0}")]
  Yaml(#[from] serde_yml::Error),
  #[error("invalid skill: {0}")]
  Validation(String),
  #[error("duplicate skill name `{0}`")]
  Duplicate(String),
  #[error("could not read skill path {path}: {source}")]
  Io {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("skill path {0} must not be a symbolic link or junction")]
  Linked(PathBuf),
  #[error("skill file {path} exceeds {limit} bytes")]
  TooLarge { path: PathBuf, limit: usize },
  #[error("skill directory contains more than {limit} YAML files")]
  TooManyFiles { limit: usize },
  #[error("skill files exceed aggregate limit of {limit} bytes")]
  AggregateTooLarge { limit: usize },
  #[error("skill file {0} is not valid UTF-8")]
  InvalidUtf8(PathBuf),
  #[error("invalid skill file {path}: {source}")]
  File {
    path: PathBuf,
    #[source]
    source: Box<SkillError>,
  },
}

impl Skill {
  pub fn from_yaml(yaml: &str) -> Result<Self, SkillError> {
    let skill: Self = serde_yml::from_str(yaml)?;
    skill.validate()?;
    Ok(skill)
  }

  pub fn validate(&self) -> Result<(), SkillError> {
    if self.name.trim().is_empty() {
      return Err(SkillError::Validation("name must not be empty".into()));
    }
    if self
      .triggers
      .iter()
      .any(|trigger| trigger.trim().is_empty())
    {
      return Err(SkillError::Validation(
        "triggers must not contain empty values".into(),
      ));
    }
    if self.triggers.iter().any(|trigger| !trigger.is_ascii()) {
      return Err(SkillError::Validation(
        "triggers must contain ASCII text only".into(),
      ));
    }
    Ok(())
  }

  pub fn matches(&self, prompt: &str) -> bool {
    self
      .triggers
      .iter()
      .any(|trigger| contains_trigger(prompt, trigger))
  }
}

#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
  skills: BTreeMap<String, Skill>,
}

impl SkillRegistry {
  pub fn new(skills: impl IntoIterator<Item = Skill>) -> Result<Self, SkillError> {
    let mut registry = Self::default();
    for skill in skills {
      skill.validate()?;
      let name = skill.name.clone();
      if registry.skills.insert(name.clone(), skill).is_some() {
        return Err(SkillError::Duplicate(name));
      }
    }
    Ok(registry)
  }

  pub fn match_prompt(&self, prompt: &str) -> Vec<&Skill> {
    self
      .skills
      .values()
      .filter(|skill| skill.matches(prompt))
      .collect()
  }

  pub fn get(&self, name: &str) -> Option<&Skill> {
    self.skills.get(name)
  }

  /// Loads `*.yaml` skills from an explicit directory root in stable filename order.
  pub fn load_from_root(root: impl AsRef<Path>) -> Result<Self, SkillError> {
    let root = root.as_ref();
    validate_unlinked_ancestors(root)?;
    let root_metadata = match fs::symlink_metadata(root) {
      Ok(metadata) => metadata,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
        return Ok(Self::default());
      }
      Err(source) => {
        return Err(SkillError::Io {
          path: root.to_path_buf(),
          source,
        });
      }
    };
    if is_link(&root_metadata) {
      return Err(SkillError::Linked(root.to_path_buf()));
    }
    let directory = open_skill_dir(root, &root_metadata)?;
    let entries = directory.entries().map_err(|source| SkillError::Io {
      path: root.to_path_buf(),
      source,
    })?;
    let mut names = Vec::with_capacity(MAX_SKILL_FILES);
    let mut aggregate_bytes = 0_usize;
    for entry in entries {
      let name = entry
        .map_err(|source| SkillError::Io {
          path: root.to_path_buf(),
          source,
        })?
        .file_name();
      let path = root.join(&name);
      if !Path::new(&name)
        .extension()
        .is_some_and(|extension| extension == "yaml")
      {
        continue;
      }
      let metadata = directory
        .symlink_metadata(&name)
        .map_err(|source| SkillError::Io {
          path: path.clone(),
          source,
        })?;
      if is_cap_link(&metadata) {
        return Err(SkillError::Linked(path));
      }
      if !metadata.is_file() {
        continue;
      }
      if metadata.len() > MAX_SKILL_FILE_BYTES as u64 {
        return Err(SkillError::TooLarge {
          path,
          limit: MAX_SKILL_FILE_BYTES,
        });
      }
      if names.len() == MAX_SKILL_FILES {
        return Err(SkillError::TooManyFiles {
          limit: MAX_SKILL_FILES,
        });
      }
      aggregate_bytes = aggregate_bytes.checked_add(metadata.len() as usize).ok_or(
        SkillError::AggregateTooLarge {
          limit: MAX_AGGREGATE_SKILL_BYTES,
        },
      )?;
      if aggregate_bytes > MAX_AGGREGATE_SKILL_BYTES {
        return Err(SkillError::AggregateTooLarge {
          limit: MAX_AGGREGATE_SKILL_BYTES,
        });
      }
      names.push(name);
    }
    names.sort();

    let mut skills = Vec::with_capacity(names.len());
    let mut read_bytes = 0_usize;
    for name in names {
      let path = root.join(&name);
      let metadata = directory
        .symlink_metadata(&name)
        .map_err(|source| SkillError::Io {
          path: path.clone(),
          source,
        })?;
      if is_cap_link(&metadata) {
        return Err(SkillError::Linked(path));
      }
      if !metadata.is_file() {
        continue;
      }
      if metadata.len() > MAX_SKILL_FILE_BYTES as u64 {
        return Err(SkillError::TooLarge {
          path,
          limit: MAX_SKILL_FILE_BYTES,
        });
      }
      let mut bytes = Vec::with_capacity(metadata.len() as usize);
      directory
        .open(&name)
        .and_then(|file| {
          file
            .take((MAX_SKILL_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
        })
        .map_err(|source| SkillError::Io {
          path: path.clone(),
          source,
        })?;
      if bytes.len() > MAX_SKILL_FILE_BYTES {
        return Err(SkillError::TooLarge {
          path,
          limit: MAX_SKILL_FILE_BYTES,
        });
      }
      read_bytes = read_bytes
        .checked_add(bytes.len())
        .ok_or(SkillError::AggregateTooLarge {
          limit: MAX_AGGREGATE_SKILL_BYTES,
        })?;
      if read_bytes > MAX_AGGREGATE_SKILL_BYTES {
        return Err(SkillError::AggregateTooLarge {
          limit: MAX_AGGREGATE_SKILL_BYTES,
        });
      }
      let yaml = String::from_utf8(bytes).map_err(|_| SkillError::InvalidUtf8(path.clone()))?;
      skills.push(Skill::from_yaml(&yaml).map_err(|source| SkillError::File {
        path,
        source: Box::new(source),
      })?);
    }
    Self::new(skills)
  }

  pub fn iter(&self) -> impl Iterator<Item = &Skill> {
    self.skills.values()
  }
}

mod fs_guard;
mod trigger;

use fs_guard::{is_cap_link, is_link, open_skill_dir, validate_unlinked_ancestors};
use trigger::contains_trigger;
