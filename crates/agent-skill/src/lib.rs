//! Typed skills, deterministic registration, and trigger matching.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct SkillRequirements {
    #[serde(default)]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillContext {
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Error)]
pub enum SkillError {
    #[error("invalid skill YAML: {0}")]
    Yaml(#[from] serde_yml::Error),
    #[error("invalid skill: {0}")]
    Validation(String),
    #[error("duplicate skill name `{0}`")]
    Duplicate(String),
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
        Ok(())
    }

    pub fn matches(&self, prompt: &str) -> bool {
        self.triggers
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
        self.skills
            .values()
            .filter(|skill| skill.matches(prompt))
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values()
    }
}

fn contains_trigger(prompt: &str, trigger: &str) -> bool {
    let prompt = prompt.to_lowercase();
    let trigger = trigger.trim().to_lowercase();
    if trigger.is_empty() {
        return false;
    }

    prompt.match_indices(&trigger).any(|(start, matched)| {
        let end = start + matched.len();
        let left_boundary = prompt[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !is_word_character(character));
        let right_boundary = prompt[end..]
            .chars()
            .next()
            .is_none_or(|character| !is_word_character(character));
        left_boundary && right_boundary
    })
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}
