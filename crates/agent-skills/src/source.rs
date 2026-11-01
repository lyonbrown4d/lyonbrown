use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Result, SkillError, paths};

/// A source description, never an authorization to fetch or execute anything.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillSource {
  Local {
    path: PathBuf,
  },
  Https {
    url: Url,
  },
  GitHub {
    owner: String,
    repository: String,
    revision: Option<String>,
    directory: Option<String>,
  },
}

impl SkillSource {
  /// GitHub shorthand accepts `owner/repo`; local relative paths must begin `./`.
  /// HTTPS resolution does not perform DNS, redirects, authentication, or I/O.
  pub fn parse(input: &str) -> Result<Self> {
    if input.chars().any(char::is_control) {
      return Err(SkillError::InvalidSource);
    }
    let input = input.trim();
    if input.is_empty() || input.chars().any(char::is_control) {
      return Err(SkillError::InvalidSource);
    }
    if input.starts_with(['/', '.']) || PathBuf::from(input).is_absolute() {
      return Ok(Self::Local {
        path: PathBuf::from(input),
      });
    }
    if !input.contains("://") {
      let parts: Vec<_> = input.split('/').collect();
      if let [owner, repository] = parts.as_slice() {
        return github(owner, repository, None, None);
      }
      return Err(SkillError::InvalidSource);
    }
    let url = Url::parse(input).map_err(|_| SkillError::InvalidSource)?;
    if url.scheme() != "https"
      || url.host_str().is_none()
      || !url.username().is_empty()
      || url.password().is_some()
      || url.fragment().is_some()
      || url.query().is_some()
    {
      return Err(SkillError::InvalidSource);
    }
    if url.host_str() != Some("github.com") {
      return Ok(Self::Https { url });
    }
    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    match parts.as_slice() {
      [owner, repository] => github(owner, repository, None, None),
      [owner, repository, "tree", revision, directory @ ..] if !directory.is_empty() => {
        github(owner, repository, Some(revision), Some(directory.join("/")))
      }
      _ => Err(SkillError::InvalidSource),
    }
  }
}

fn github(
  owner: &str,
  repository: &str,
  revision: Option<&str>,
  directory: Option<String>,
) -> Result<SkillSource> {
  let repository = repository.strip_suffix(".git").unwrap_or(repository);
  for value in [owner, repository].into_iter().chain(revision) {
    if value.is_empty()
      || !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
      || matches!(value, "." | "..")
    {
      return Err(SkillError::InvalidSource);
    }
  }
  if let Some(path) = &directory {
    paths::relative(path, 16)?;
  }
  Ok(SkillSource::GitHub {
    owner: owner.into(),
    repository: repository.into(),
    revision: revision.map(str::to_owned),
    directory,
  })
}
