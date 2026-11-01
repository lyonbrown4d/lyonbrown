//! Encrypted local storage for credentials owned by a host service.
//!
//! The caller owns authentication, upload consent, operation intents, receipts,
//! deletion policy and configuration binding. The dedicated root must be under
//! a trusted user data directory. A copied store includes its decryption key;
//! this protects files at rest, not against the owning OS user or host compromise.

mod crypto;
mod files;
mod store;

pub use secrecy::{ExposeSecret, SecretString};
pub use store::SecretStore;

use serde::Serialize;

pub const MAX_SECRET_BYTES: usize = 16 * 1024;

/// Safe for protocol responses; no information derived from the secret value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SecretMetadata {
  pub id: String,
  pub encryption_version: u32,
}

impl SecretMetadata {
  pub(crate) fn new(id: &str) -> Self {
    Self {
      id: id.into(),
      encryption_version: 1,
    }
  }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
  #[error("credential identifier is invalid")]
  InvalidId,
  #[error("credential value is empty, invalid, or exceeds the size limit")]
  InvalidSecret,
  #[error("credential storage contains an unsupported filesystem entry")]
  UnsupportedEntry,
  #[error("credential storage permissions are not private")]
  Permissions,
  #[error("credential master key is missing or invalid")]
  InvalidKey,
  #[error("credential record is invalid or unsupported")]
  InvalidRecord,
  #[error("credential record authentication failed")]
  Authentication,
  #[error("operating system randomness is unavailable")]
  Randomness,
  #[error("credential storage operation failed")]
  Io(#[source] std::io::Error),
}

impl From<std::io::Error> for SecretError {
  fn from(value: std::io::Error) -> Self {
    Self::Io(value)
  }
}

pub type Result<T> = std::result::Result<T, SecretError>;

pub(crate) fn validate_id(id: &str) -> Result<()> {
  let reserved = matches!(id, "con" | "prn" | "aux" | "nul")
    || ((id.starts_with("com") || id.starts_with("lpt"))
      && id.len() == 4
      && matches!(id.as_bytes()[3], b'1'..=b'9'));
  if id.is_empty()
    || id.len() > 64
    || reserved
    || !id
      .bytes()
      .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_'))
  {
    return Err(SecretError::InvalidId);
  }
  Ok(())
}
