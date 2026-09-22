//! Typed agent profiles and filesystem loading.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProfile {
    pub id: String,
    #[serde(default)]
    pub persona: Option<String>,
    #[serde(default)]
    pub communication_style: Option<String>,
    #[serde(default)]
    pub working_style: WorkingStyle,
    #[serde(default)]
    pub model: ModelPreferences,
    #[serde(default)]
    pub coding: CodingPreferences,
    #[serde(default)]
    pub tool_preferences: Vec<String>,
    #[serde(default)]
    pub reasoning_policy: Option<String>,
    #[serde(default)]
    pub privacy_policy: Option<String>,
    #[serde(default)]
    pub default_skills: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkingStyle {
    #[serde(default)]
    pub prefer_existing_libraries: bool,
    #[serde(default)]
    pub avoid_reinventing_wheels: bool,
    #[serde(default)]
    pub inspect_before_editing: bool,
    #[serde(default)]
    pub run_tests_after_changes: bool,
    #[serde(default)]
    pub architecture_first: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPreferences {
    #[serde(default)]
    pub prefer_local: bool,
    #[serde(default)]
    pub remote_escalation: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodingPreferences {
    #[serde(default)]
    pub avoid_unnecessary_abstraction: bool,
    #[serde(default)]
    pub prefer_small_reviewable_changes: bool,
}

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("invalid profile YAML: {0}")]
    Yaml(#[from] serde_yml::Error),
    #[error("invalid profile: {0}")]
    Validation(String),
    #[error("could not read profile {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl AgentProfile {
    pub fn from_yaml(yaml: &str) -> Result<Self, ProfileError> {
        let profile: Self = serde_yml::from_str(yaml)?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), ProfileError> {
        validate_id(&self.id)
    }

    pub fn builtin_default() -> Self {
        Self {
            id: "default-dev".into(),
            persona: Some("Pragmatic software engineer".into()),
            communication_style: None,
            working_style: WorkingStyle {
                prefer_existing_libraries: true,
                avoid_reinventing_wheels: true,
                inspect_before_editing: true,
                run_tests_after_changes: true,
                architecture_first: true,
            },
            model: ModelPreferences {
                prefer_local: true,
                remote_escalation: true,
            },
            coding: CodingPreferences {
                avoid_unnecessary_abstraction: true,
                prefer_small_reviewable_changes: true,
            },
            tool_preferences: Vec::new(),
            reasoning_policy: None,
            privacy_policy: None,
            default_skills: Vec::new(),
        }
    }
}

impl Default for AgentProfile {
    fn default() -> Self {
        Self::builtin_default()
    }
}

pub struct ProfileLoader {
    home_root: PathBuf,
}

impl ProfileLoader {
    pub fn new(home_root: impl AsRef<Path>) -> Self {
        Self {
            home_root: home_root.as_ref().to_path_buf(),
        }
    }

    pub fn load(&self, id: &str) -> Result<AgentProfile, ProfileError> {
        validate_id(id)?;
        let path = self
            .home_root
            .join(".agent")
            .join("profiles")
            .join(format!("{id}.yaml"));
        let yaml = fs::read_to_string(&path).map_err(|source| ProfileError::Io {
            path: path.clone(),
            source,
        })?;
        let profile = AgentProfile::from_yaml(&yaml)?;
        if profile.id != id {
            return Err(ProfileError::Validation(format!(
                "profile id `{}` does not match requested id `{id}`",
                profile.id
            )));
        }
        Ok(profile)
    }
}

pub fn load_profile(id: &str) -> Result<AgentProfile, ProfileError> {
    let home = dirs::home_dir()
        .ok_or_else(|| ProfileError::Validation("home directory is unavailable".into()))?;
    ProfileLoader::new(home).load(id)
}

fn validate_id(id: &str) -> Result<(), ProfileError> {
    if id.is_empty()
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(ProfileError::Validation(
            "profile id must contain only ASCII letters, digits, '-' or '_'".into(),
        ));
    }
    Ok(())
}
