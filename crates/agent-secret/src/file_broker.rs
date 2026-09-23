use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Component, Path};
use std::sync::Arc;

use agent_core::SecretRef;
use async_trait::async_trait;
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};

#[cfg(unix)]
use cap_std::fs::{OpenOptionsExt, Permissions, PermissionsExt};

use crate::redact::ResolvedSecrets;
use crate::{SecretBroker, SecretError};

#[derive(Clone)]
pub struct FileSecretBroker {
  directory: Arc<Dir>,
  resolved: ResolvedSecrets,
}

impl FileSecretBroker {
  pub fn new(root: impl AsRef<Path>) -> Result<Self, SecretError> {
    let root = root.as_ref();
    std::fs::create_dir_all(root).map_err(|error| {
      SecretError::Provider(format!("failed to create secret directory: {error}"))
    })?;
    #[cfg(unix)]
    std::fs::set_permissions(root, std::os::unix::fs::PermissionsExt::from_mode(0o700)).map_err(
      |error| SecretError::Provider(format!("failed to secure secret directory: {error}")),
    )?;
    let directory = Dir::open_ambient_dir(root, ambient_authority()).map_err(|error| {
      SecretError::Provider(format!("failed to open secret directory: {error}"))
    })?;
    Ok(Self {
      directory: Arc::new(directory),
      resolved: ResolvedSecrets::default(),
    })
  }

  pub fn store(&self, name: &str, value: &str) -> Result<(), SecretError> {
    let reference = SecretRef::file(name);
    validate_name(&reference)?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = self.directory.open_with(name, &options).map_err(|error| {
      SecretError::Provider(format!("failed to store secret {reference}: {error}"))
    })?;
    #[cfg(unix)]
    file
      .set_permissions(Permissions::from_mode(0o600))
      .map_err(|error| {
        SecretError::Provider(format!("failed to secure secret {reference}: {error}"))
      })?;
    file.write_all(value.as_bytes()).map_err(|error| {
      SecretError::Provider(format!("failed to store secret {reference}: {error}"))
    })
  }
}

#[async_trait]
impl SecretBroker for FileSecretBroker {
  async fn resolve(&self, reference: &SecretRef) -> Result<String, SecretError> {
    if reference.parts().0 != "file" {
      return Err(SecretError::NotFound(reference.clone()));
    }
    let name = validate_name(reference)?;
    let mut file = self.directory.open(name).map_err(|error| {
      if error.kind() == std::io::ErrorKind::NotFound {
        SecretError::NotFound(reference.clone())
      } else {
        SecretError::Provider(format!("failed to read secret {reference}: {error}"))
      }
    })?;
    let mut encoded = String::new();
    file.read_to_string(&mut encoded).map_err(|error| {
      SecretError::Provider(format!("failed to read secret {reference}: {error}"))
    })?;
    let value = decode_file_value(&encoded);
    self.resolved.register(&value);
    Ok(value)
  }

  fn redact(&self, text: &str) -> String {
    self.resolved.redact(text)
  }
}

fn validate_name(reference: &SecretRef) -> Result<&str, SecretError> {
  let (kind, name) = reference.parts();
  let path = Path::new(name);
  let mut components = path.components();
  let valid = kind == "file"
    && !name.is_empty()
    && matches!(components.next(), Some(Component::Normal(component)) if component != OsStr::new(""))
    && components.next().is_none();
  if !valid {
    return Err(SecretError::InvalidReference(reference.clone()));
  }
  Ok(name)
}

fn decode_file_value(encoded: &str) -> String {
  let encoded = encoded.trim_end_matches(['\r', '\n']);
  if let Some((name, value)) = encoded.split_once('=')
    && valid_assignment_name(name)
  {
    return value.to_owned();
  }
  encoded.to_owned()
}

fn valid_assignment_name(name: &str) -> bool {
  let mut characters = name.chars();
  matches!(characters.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
    && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}
